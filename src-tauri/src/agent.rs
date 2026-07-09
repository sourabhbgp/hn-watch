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

/// Env override for tests / live-verification: point at a fake `claude` script.
const CLAUDE_BIN_ENV: &str = "HN_WATCH_CLAUDE_BIN";

/// Resolved absolute path to the `claude` binary. Honors `HN_WATCH_CLAUDE_BIN`
/// (uncached, for tests), else the cached PATH/common-dir resolution, else the
/// bare name "claude".
fn claude_bin() -> String {
    if let Ok(p) = std::env::var(CLAUDE_BIN_ENV) {
        if !p.is_empty() {
            return p;
        }
    }
    static BIN: OnceLock<String> = OnceLock::new();
    BIN.get_or_init(|| find_claude(claude_candidates()).unwrap_or_else(|| "claude".to_string()))
        .clone()
}

/// True when a real `claude` binary exists (not the bare-name fallback). Drives
/// the preflight "Missing" state before we ever try to spawn.
pub fn claude_present() -> bool {
    if let Ok(p) = std::env::var(CLAUDE_BIN_ENV) {
        if !p.is_empty() {
            return std::path::Path::new(&p).exists();
        }
    }
    find_claude(claude_candidates()).is_some()
}

/// Base command carrying the sandbox that keeps any `claude` call from reading
/// the user's files / triggering a macOS TCC prompt: run from the temp dir, override
/// $PWD (claude walks up from $PWD, not getcwd()), null stdin (else it waits ~3s for
/// piped input), kill on drop. Callers append their own args.
fn claude_command() -> tokio::process::Command {
    let workdir = std::env::temp_dir();
    let mut cmd = tokio::process::Command::new(claude_bin());
    cmd.current_dir(&workdir)
        .env("PWD", &workdir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    cmd
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
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        claude_command().arg("-p").arg("--safe-mode").arg(&prompt).output(),
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
