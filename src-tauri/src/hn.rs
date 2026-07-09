use serde::Deserialize;

const HITS_PER_PAGE: usize = 100;
const MAX_PAGES: usize = 10;

/// Algolia numericFilters clause: only stories submitted at/after `since`.
fn numeric_filter(since: i64) -> String {
    format!("created_at_i>={since}")
}

#[derive(Debug, Clone)]
pub struct HnItem {
    pub hn_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub points: i64,
    pub num_comments: i64,
    pub created_at: i64,
}

#[derive(Deserialize)]
struct AlgoliaResponse {
    hits: Vec<AlgoliaHit>,
}

#[derive(Deserialize)]
struct AlgoliaHit {
    #[serde(rename = "objectID")]
    object_id: String,
    title: Option<String>,
    url: Option<String>,
    points: Option<i64>,
    num_comments: Option<i64>,
    created_at_i: Option<i64>,
}

fn domain_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string()
}

pub fn parse_algolia(body: &str) -> Vec<HnItem> {
    let resp: AlgoliaResponse = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    resp.hits
        .into_iter()
        .filter_map(|h| {
            let title = h.title?;
            // Self/Ask/Show posts without an external url point at the HN item.
            let url = h
                .url
                .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", h.object_id));
            let domain = domain_of(&url);
            Some(HnItem {
                hn_id: h.object_id,
                title,
                url,
                domain,
                points: h.points.unwrap_or(0),
                num_comments: h.num_comments.unwrap_or(0),
                created_at: h.created_at_i.unwrap_or(0),
            })
        })
        .collect()
}

/// Fetch every `story` submitted at/after `since`, newest-first, paginating until a
/// short page (the last one) or the `MAX_PAGES` safety cap. Cross-page duplicates are
/// possible (new arrivals push items to later pages) and are dropped downstream by
/// `dedupe_by_hn_id` / `seen` / `UNIQUE`; stories are never deleted, so nothing is skipped.
pub async fn fetch_since(since: i64) -> Result<Vec<HnItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("hn client build failed: {e}"))?;
    let filter = numeric_filter(since);
    let hpp = HITS_PER_PAGE.to_string();
    let mut all: Vec<HnItem> = Vec::new();
    for page in 0..MAX_PAGES {
        let page_s = page.to_string();
        let body = client
            .get("https://hn.algolia.com/api/v1/search_by_date")
            .query(&[
                ("tags", "story"),
                ("numericFilters", filter.as_str()),
                ("hitsPerPage", hpp.as_str()),
                ("page", page_s.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("hn request failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("hn read failed: {e}"))?;
        let items = parse_algolia(&body);
        let got = items.len();
        all.extend(items);
        if got < HITS_PER_PAGE {
            break; // short page → last page for this window
        }
        if page + 1 >= MAX_PAGES {
            eprintln!(
                "[hn-watch] fetch_since hit MAX_PAGES ({MAX_PAGES}) cap; \
                 window may be truncated (since={since})"
            );
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits_and_derives_domain() {
        let body = r#"{
          "hits": [
            {"objectID":"1","title":"A tool","url":"https://www.example.dev/a","points":10,"num_comments":3,"created_at_i":1700000000},
            {"objectID":"2","title":"Ask HN: something","points":5}
          ]
        }"#;
        let items = parse_algolia(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].hn_id, "1");
        assert_eq!(items[0].domain, "example.dev"); // www. stripped
        assert_eq!(items[0].created_at, 1_700_000_000); // parsed from created_at_i
        assert_eq!(items[1].url, "https://news.ycombinator.com/item?id=2"); // fallback url
        assert_eq!(items[1].created_at, 0); // missing created_at_i defaults to 0
    }

    #[test]
    fn numeric_filter_formats_since() {
        assert_eq!(numeric_filter(1_700_000_000), "created_at_i>=1700000000");
    }

    #[test]
    fn bad_json_yields_empty() {
        assert!(parse_algolia("not json").is_empty());
    }
}
