use crate::hn::HnItem;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Shared agent runtime bound: monitor ticks (one call each) and the future
/// dig-deeper swarm (many at once) both acquire from this single semaphore.
fn agent_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(4))
}

/// First candidate path that exists, as a string; None if none exist.
fn find_claude(candidates: impl IntoIterator<Item = PathBuf>) -> Option<String> {
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Where `claude` might live: every dir already on PATH, then common install
/// locations. A GUI-launched macOS app inherits a minimal PATH that omits
/// ~/.local/bin etc., so we can't rely on PATH resolution alone.
fn claude_candidates() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            dirs.push(dir.join("claude"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin/claude"));
        dirs.push(home.join(".bun/bin/claude"));
        dirs.push(home.join("bin/claude"));
    }
    for p in [
        "/opt/homebrew/bin/claude",
        "/usr/local/bin/claude",
        "/usr/bin/claude",
    ] {
        dirs.push(PathBuf::from(p));
    }
    dirs
}

/// Resolved absolute path to the `claude` binary, computed once. Falls back to
/// the bare name "claude" (PATH resolution) if nothing is found.
fn claude_bin() -> String {
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BIN.get_or_init(|| find_claude(claude_candidates()).unwrap_or_else(|| "claude".to_string()))
        .clone()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Verdict {
    pub hn_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reason: String,
}

pub fn build_prompt(user_prompt: &str, items: &[HnItem]) -> String {
    let list: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "hn_id": i.hn_id,
                "title": i.title,
                "url": i.url,
                "points": i.points,
            })
        })
        .collect();
    let items_json = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into());
    format!(
        "You are a filter for a Hacker News watcher. The user cares about:\n\
         \"{user_prompt}\"\n\n\
         Here are recent HN stories as a JSON array:\n{items_json}\n\n\
         Return ONLY a JSON array (no prose, no markdown fences) of the stories that genuinely \
         match the user's interest. Each element must be an object with exactly these keys: \
         \"hn_id\" (string, copied from the input), \"summary\" (one or two sentences on what \
         the story is), and \"reason\" (one sentence on why it matches the interest). \
         If nothing matches, return []."
    )
}

/// Pull the first JSON array out of the model's response and parse it.
pub fn parse_verdict(text: &str) -> Vec<Verdict> {
    let start = match text.find('[') {
        Some(s) => s,
        None => return Vec::new(),
    };
    let end = match text.rfind(']') {
        Some(e) if e > start => e,
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<Verdict>>(&text[start..=end]).unwrap_or_default()
}

pub async fn judge(user_prompt: &str, items: &[HnItem]) -> Result<Vec<Verdict>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let prompt = build_prompt(user_prompt, items);
    let _permit = agent_sem()
        .acquire()
        .await
        .map_err(|e| format!("semaphore closed: {e}"))?;
    // This judge call is pure text-in / JSON-out — it must never read the user's
    // files. Claude Code otherwise treats its surroundings as a project workspace
    // and, for a Finder-launched bundle sitting under ~/Desktop, that triggers a
    // macOS "access your Desktop folder" TCC prompt that blocks the tick. Three
    // things keep it fully sandboxed while preserving the CLI's own keychain auth:
    //   --safe-mode : start with all customizations off — no CLAUDE.md / memory /
    //                 plugin / hook / MCP discovery (keeps OAuth/keychain auth,
    //                 unlike --bare which forces an API key).
    //   current_dir : run from the temp dir, not the inherited (Desktop) cwd.
    //   env("PWD")  : Claude walks up from $PWD, NOT getcwd() — current_dir alone
    //                 leaves the inherited $PWD (~/Desktop...) in place, so we must
    //                 override it too or the tree-walk still reaches Desktop.
    // stdin(null): claude -p otherwise waits ~3s for piped stdin on every call.
    let workdir = std::env::temp_dir();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::process::Command::new(claude_bin())
            .arg("-p")
            .arg("--safe-mode")
            .arg(&prompt)
            .current_dir(&workdir)
            .env("PWD", &workdir)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "claude timed out after 90s".to_string())?
    .map_err(|e| format!("failed to spawn claude: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "claude exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_verdict(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_amid_prose() {
        let text = "Sure! Here are the matches:\n\
            [{\"hn_id\":\"1\",\"summary\":\"A tool\",\"reason\":\"matches\"}]\nHope that helps.";
        let v = parse_verdict(text);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].hn_id, "1");
        assert_eq!(v[0].summary, "A tool");
    }

    #[test]
    fn empty_array_and_garbage() {
        assert_eq!(parse_verdict("[]").len(), 0);
        assert_eq!(parse_verdict("no json here").len(), 0);
        assert_eq!(parse_verdict("[broken").len(), 0);
    }

    #[test]
    fn prompt_contains_prompt_and_ids() {
        let items = vec![HnItem {
            hn_id: "42".into(), title: "Rust".into(), url: "u".into(),
            domain: "d".into(), points: 1, num_comments: 1,
        }];
        let p = build_prompt("rust async", &items);
        assert!(p.contains("rust async"));
        assert!(p.contains("\"42\""));
    }

    #[test]
    fn find_claude_picks_first_existing() {
        // A path we know exists on any unix: /bin/sh. Use it as a stand-in binary.
        let existing = std::path::PathBuf::from("/bin/sh");
        let missing = std::path::PathBuf::from("/no/such/dir/claude");
        // missing first, then existing -> returns the existing one
        let got = find_claude(vec![missing.clone(), existing.clone()]);
        assert_eq!(got, Some("/bin/sh".to_string()));
        // nothing exists -> None
        assert_eq!(find_claude(vec![missing]), None);
    }
}
