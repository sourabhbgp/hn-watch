use crate::db::FeedItemContext;
use crate::hn::HnItem;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::Semaphore;
use uuid::Uuid;

/// Reserved-pool concurrency for the shared `claude` runtime. Monitor ticks draw only
/// from `tick_sem`; the dig-deeper swarm (planner, each worker, synthesis) draws only
/// from `swarm_sem`. Strict separation (no overflow) means an interactive swarm never
/// queues behind background ticks, and ticks are never blocked by an in-flight swarm.
/// Both are FIFO-fair, so a third caller of a full pool simply waits its turn.
pub const TICK_PERMITS: usize = 2;
pub const SWARM_PERMITS: usize = 5;

fn tick_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(TICK_PERMITS))
}

#[allow(dead_code)]
fn swarm_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(SWARM_PERMITS))
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

/// Number of investigative angles a swarm may run — enforced client- and server-side.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub const MIN_ANGLES: usize = 2;
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub const MAX_ANGLES: usize = 5;
/// Icon pool, assigned to angles by index (the LLM never emits an emoji).
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub const ANGLE_ICONS: [&str; 5] = ["🏢", "🔧", "📊", "🕵️", "🧭"];

/// One investigative angle: what a single swarm worker will look into.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAngle {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub focus: String,
}

/// Raw planner output before clamping/icon assignment.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
#[derive(serde::Deserialize)]
struct RawAngle {
    #[serde(default)]
    label: String,
    #[serde(default)]
    focus: String,
}

/// Build a `PlannedAngle` from a label+focus, assigning the icon at `index` (mod pool size)
/// and a fresh uuid.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
fn angle_at(index: usize, label: String, focus: String) -> PlannedAngle {
    PlannedAngle {
        id: Uuid::new_v4().to_string(),
        icon: ANGLE_ICONS[index % ANGLE_ICONS.len()].to_string(),
        label,
        focus,
    }
}

/// Fallback angle set when the planner fails or returns too few usable angles.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub fn default_angles() -> Vec<PlannedAngle> {
    [
        ("Company & people", "Who is behind the story — founders, team, backers, org."),
        ("Tech & how it works", "The underlying technology and how it actually works."),
        ("Market & rivals", "The market, competitors, and how this compares."),
        ("Skeptic / risks", "Risks, criticisms, and reasons for skepticism."),
    ]
    .iter()
    .enumerate()
    .map(|(i, (l, f))| angle_at(i, (*l).to_string(), (*f).to_string()))
    .collect()
}

#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub fn build_plan_prompt(ctx: &FeedItemContext) -> String {
    format!(
        "You are planning a research swarm to dig deeper into one Hacker News story, \
         for a user whose monitor is interested in:\n\"{prompt}\"\n\n\
         Story: \"{title}\" ({domain}, {url})\n\
         Why it matched: {reason}\n\
         Initial summary: {summary}\n\n\
         Decide between 2 and 5 distinct investigative angles for THIS SPECIFIC STORY. \
         Each angle should pull from genuinely different context or sources — do not force a \
         generic template if it doesn't fit.\n\n\
         Return ONLY a JSON array (2 to 5 elements, no prose, no markdown fences) of objects \
         with exactly these keys: \"label\" (short 2-4 word angle name) and \"focus\" (one \
         sentence telling an investigator exactly what to look into).",
        prompt = ctx.monitor_prompt,
        title = ctx.title,
        domain = ctx.domain,
        url = ctx.url,
        reason = ctx.reason,
        summary = ctx.summary,
    )
}

/// Parse the planner's JSON array into clamped, icon-assigned angles. Tolerant like
/// `parse_verdict` (finds the first `[ … ]`). Drops entries with an empty label/focus;
/// if fewer than `MIN_ANGLES` survive (or the text is unparseable), returns `default_angles()`;
/// truncates to `MAX_ANGLES`.
#[allow(dead_code)] // consumed by Task 6 (plan_angles)
pub fn parse_plan(text: &str) -> Vec<PlannedAngle> {
    let slice = match (text.find('['), text.rfind(']')) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => return default_angles(),
    };
    let raw: Vec<RawAngle> = serde_json::from_str(slice).unwrap_or_default();
    let cleaned: Vec<PlannedAngle> = raw
        .into_iter()
        .filter(|a| !a.label.trim().is_empty() && !a.focus.trim().is_empty())
        .take(MAX_ANGLES)
        .enumerate()
        .map(|(i, a)| angle_at(i, a.label.trim().to_string(), a.focus.trim().to_string()))
        .collect();
    if cleaned.len() < MIN_ANGLES {
        default_angles()
    } else {
        cleaned
    }
}

/// A classified failure of a single `claude` call. `code()` is stable and drives
/// paused-vs-error + global health; `message()` is the friendly UI copy.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    NotFound,
    NotAuthenticated,
    Timeout,
    Failed(String),
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            AgentError::NotFound => "claude_missing",
            AgentError::NotAuthenticated => "claude_auth",
            AgentError::Timeout => "claude_timeout",
            AgentError::Failed(_) => "claude_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AgentError::NotFound => "Claude Code was not found on this machine".into(),
            AgentError::NotAuthenticated => "Claude Code isn't logged in".into(),
            AgentError::Timeout => "Claude timed out".into(),
            AgentError::Failed(s) => format!("Claude failed: {s}"),
        }
    }
}

/// Best-effort: does claude's stderr indicate a login / auth problem?
fn is_auth_failure(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("not logged in")
        || s.contains("/login")
        || s.contains("please run")
        || s.contains("authenticate")
        || s.contains("invalid api key")
        || s.contains("unauthorized")
}

/// Global Claude availability, seeded at startup and kept live by ticks.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeHealth {
    Ok,
    Missing,
    NotAuthenticated,
}

impl ClaudeHealth {
    pub fn code(&self) -> &'static str {
        match self {
            ClaudeHealth::Ok => "ok",
            ClaudeHealth::Missing => "missing",
            ClaudeHealth::NotAuthenticated => "notAuthenticated",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ClaudeHealth::Ok => String::new(),
            ClaudeHealth::Missing => {
                "Claude Code not found — HN Watch needs it to judge stories. \
                 Install Claude Code, then Re-check."
                    .into()
            }
            ClaudeHealth::NotAuthenticated => {
                "Claude Code isn't logged in — run `claude` in a terminal to log in, \
                 then Re-check."
                    .into()
            }
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, ClaudeHealth::Ok)
    }
}

/// Compute the next global health from a tick outcome. A tick that actually ran the
/// agent and succeeded → Ok (self-heal). A successful *early return* that never called
/// the agent → unchanged (it is not evidence Claude works). `claude_missing`/`claude_auth`
/// errors set the corresponding down-state; any other error leaves health unchanged.
pub fn next_claude_health(
    current: &ClaudeHealth,
    agent_ran: bool,
    error_code: Option<&str>,
) -> ClaudeHealth {
    match error_code {
        None if agent_ran => ClaudeHealth::Ok,
        None => current.clone(),
        Some("claude_missing") => ClaudeHealth::Missing,
        Some("claude_auth") => ClaudeHealth::NotAuthenticated,
        Some(_) => current.clone(),
    }
}

/// Pure: map `claude auth status --json` output to a health state.
/// non-zero exit → NotAuthenticated (logged-out exits 1); exit 0 with
/// `{"loggedIn": false}` → NotAuthenticated; anything else on exit 0 → Ok
/// (unparseable output must not false-alarm).
pub fn classify_auth(success: bool, stdout: &str) -> ClaudeHealth {
    if !success {
        return ClaudeHealth::NotAuthenticated;
    }
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(v) if v.get("loggedIn").and_then(|b| b.as_bool()) == Some(false) => {
            ClaudeHealth::NotAuthenticated
        }
        _ => ClaudeHealth::Ok,
    }
}

/// Startup / re-check probe. Cheap: `claude auth status --json` makes NO model
/// call. Binary absent → Missing without spawning; probe that itself fails to
/// run / times out → Ok (don't false-alarm — real ticks surface genuine errors).
pub async fn preflight() -> ClaudeHealth {
    if !claude_present() {
        return ClaudeHealth::Missing;
    }
    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        claude_command().arg("auth").arg("status").arg("--json").output(),
    )
    .await;
    match probe {
        Ok(Ok(output)) => {
            classify_auth(output.status.success(), &String::from_utf8_lossy(&output.stdout))
        }
        _ => ClaudeHealth::Ok,
    }
}

pub async fn judge(user_prompt: &str, items: &[HnItem]) -> Result<Vec<Verdict>, AgentError> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let prompt = build_prompt(user_prompt, items);
    let _permit = tick_sem()
        .acquire()
        .await
        .map_err(|e| AgentError::Failed(format!("semaphore closed: {e}")))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        claude_command()
            .arg("-p")
            .arg("--safe-mode")
            .arg("--model")
            .arg("claude-sonnet-5")
            .arg(&prompt)
            .output(),
    )
    .await
    .map_err(|_| AgentError::Timeout)?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AgentError::NotFound
        } else {
            AgentError::Failed(format!("failed to spawn claude: {e}"))
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if is_auth_failure(&stderr) {
            AgentError::NotAuthenticated
        } else {
            AgentError::Failed(format!("claude exited with status {}: {stderr}", output.status))
        });
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
            domain: "d".into(), points: 1, num_comments: 1, created_at: 1,
        }];
        let p = build_prompt("rust async", &items);
        assert!(p.contains("rust async"));
        assert!(p.contains("\"42\""));
    }

    #[test]
    fn auth_failure_stderr_detected() {
        assert!(is_auth_failure("Not logged in · Please run /login"));
        assert!(is_auth_failure("Invalid API key"));
        assert!(is_auth_failure("Unauthorized"));
        assert!(!is_auth_failure("network error: connection timed out"));
    }

    #[test]
    fn agent_error_codes_and_messages() {
        assert_eq!(AgentError::NotFound.code(), "claude_missing");
        assert_eq!(AgentError::NotAuthenticated.code(), "claude_auth");
        assert_eq!(AgentError::Timeout.code(), "claude_timeout");
        assert_eq!(AgentError::Failed("x".into()).code(), "claude_error");
        assert!(AgentError::Timeout.message().contains("timed out"));
        assert!(AgentError::NotAuthenticated.message().contains("logged in"));
    }

    #[test]
    fn classify_auth_states() {
        assert_eq!(classify_auth(true, r#"{"loggedIn":true}"#), ClaudeHealth::Ok);
        assert_eq!(classify_auth(true, r#"{"loggedIn":false}"#), ClaudeHealth::NotAuthenticated);
        assert_eq!(classify_auth(false, ""), ClaudeHealth::NotAuthenticated);
        // unparseable stdout on a zero exit must NOT false-alarm
        assert_eq!(classify_auth(true, "not json at all"), ClaudeHealth::Ok);
    }

    #[test]
    fn next_claude_health_transitions() {
        // real success clears a down-state
        assert_eq!(next_claude_health(&ClaudeHealth::Missing, true, None), ClaudeHealth::Ok);
        // early-return success must NOT clear a down-state (the live bug)
        assert_eq!(next_claude_health(&ClaudeHealth::Missing, false, None), ClaudeHealth::Missing);
        assert_eq!(
            next_claude_health(&ClaudeHealth::NotAuthenticated, false, None),
            ClaudeHealth::NotAuthenticated
        );
        // missing / auth errors set the down-state
        assert_eq!(next_claude_health(&ClaudeHealth::Ok, true, Some("claude_missing")), ClaudeHealth::Missing);
        assert_eq!(next_claude_health(&ClaudeHealth::Ok, true, Some("claude_auth")), ClaudeHealth::NotAuthenticated);
        // transient errors leave health unchanged
        assert_eq!(next_claude_health(&ClaudeHealth::Ok, true, Some("claude_timeout")), ClaudeHealth::Ok);
        assert_eq!(next_claude_health(&ClaudeHealth::Missing, true, Some("hn_error")), ClaudeHealth::Missing);
    }

    #[test]
    fn claude_health_projection() {
        assert_eq!(ClaudeHealth::Ok.code(), "ok");
        assert_eq!(ClaudeHealth::Missing.code(), "missing");
        assert_eq!(ClaudeHealth::NotAuthenticated.code(), "notAuthenticated");
        assert!(ClaudeHealth::Ok.is_ok());
        assert!(!ClaudeHealth::Missing.is_ok());
        assert!(ClaudeHealth::Missing.message().contains("not found"));
        assert!(ClaudeHealth::NotAuthenticated.message().contains("logged in"));
        assert!(ClaudeHealth::Ok.message().is_empty());
    }

    #[test]
    fn pools_are_independent() {
        tauri::async_runtime::block_on(async {
            // Exhaust the tick pool entirely...
            let mut held = Vec::new();
            for _ in 0..TICK_PERMITS {
                held.push(tick_sem().acquire().await.unwrap());
            }
            assert_eq!(tick_sem().available_permits(), 0);
            // ...the swarm pool is untouched and still fully available.
            assert_eq!(swarm_sem().available_permits(), SWARM_PERMITS);
            let _s = swarm_sem()
                .try_acquire()
                .expect("swarm pool must be free while ticks are saturated");
            drop(held);
        });
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

    fn ctx() -> crate::db::FeedItemContext {
        crate::db::FeedItemContext {
            title: "Orbital (YC W26) files your taxes".into(),
            url: "https://news.ycombinator.com/item?id=1".into(),
            domain: "news.ycombinator.com".into(),
            summary: "an agent that prepares tax returns".into(),
            reason: "AI-agent startup launch".into(),
            monitor_prompt: "AI-agent startup launches".into(),
        }
    }

    #[test]
    fn build_plan_prompt_contains_story_and_interest() {
        let p = build_plan_prompt(&ctx());
        assert!(p.contains("AI-agent startup launches")); // monitor prompt
        assert!(p.contains("Orbital (YC W26) files your taxes")); // title
        assert!(p.contains("2 and 5")); // the 2–5 instruction
    }

    #[test]
    fn parse_plan_accepts_valid_and_assigns_icons_by_index() {
        let text = r#"[
          {"label":"Company","focus":"who founded it"},
          {"label":"Tech","focus":"how it works"},
          {"label":"Market","focus":"competitors"}
        ]"#;
        let angles = parse_plan(text);
        assert_eq!(angles.len(), 3);
        assert_eq!(angles[0].label, "Company");
        assert_eq!(angles[0].icon, ANGLE_ICONS[0]);
        assert_eq!(angles[1].icon, ANGLE_ICONS[1]);
        assert!(!angles[0].id.is_empty()); // uuid assigned
    }

    #[test]
    fn parse_plan_drops_empty_entries_then_may_fall_back() {
        // Only one valid entry survives filtering (< MIN_ANGLES) -> default set.
        let text = r#"[{"label":"","focus":"x"},{"label":"Only","focus":""},{"label":"Keep","focus":"real"}]"#;
        let angles = parse_plan(text);
        assert_eq!(angles.len(), default_angles().len()); // fell back
    }

    #[test]
    fn parse_plan_truncates_to_max() {
        let text = r#"[
          {"label":"a","focus":"1"},{"label":"b","focus":"2"},{"label":"c","focus":"3"},
          {"label":"d","focus":"4"},{"label":"e","focus":"5"},{"label":"f","focus":"6"},{"label":"g","focus":"7"}
        ]"#;
        assert_eq!(parse_plan(text).len(), MAX_ANGLES); // 5
    }

    #[test]
    fn parse_plan_garbage_falls_back() {
        assert_eq!(parse_plan("not json").len(), default_angles().len());
        assert_eq!(parse_plan("[]").len(), default_angles().len());
    }

    #[test]
    fn default_angles_are_wellformed_and_distinct() {
        let a = default_angles();
        assert_eq!(a.len(), 4);
        assert!(a.iter().all(|x| !x.label.is_empty() && !x.focus.is_empty() && !x.id.is_empty()));
        // distinct icons for the first 4
        assert_eq!(a[0].icon, ANGLE_ICONS[0]);
        assert_eq!(a[3].icon, ANGLE_ICONS[3]);
    }
}
