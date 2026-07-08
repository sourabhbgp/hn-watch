use crate::hn::HnItem;
use serde::Deserialize;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Shared agent runtime bound: monitor ticks (one call each) and the future
/// dig-deeper swarm (many at once) both acquire from this single semaphore.
fn agent_sem() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(4))
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
    // stdin(null): claude -p otherwise waits ~3s for piped stdin each call
    // ("Warning: no stdin data received in 3s"). We pass the prompt as an arg.
    let output = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .stdin(std::process::Stdio::null())
        .output()
        .await
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
}
