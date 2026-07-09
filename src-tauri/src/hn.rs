use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct HnItem {
    pub hn_id: String,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub points: i64,
    pub num_comments: i64,
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
            })
        })
        .collect()
}

pub async fn fetch_recent(limit: usize) -> Result<Vec<HnItem>, String> {
    let url = format!(
        "https://hn.algolia.com/api/v1/search_by_date?tags=story&hitsPerPage={}",
        limit
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("hn client build failed: {e}"))?;
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("hn request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("hn read failed: {e}"))?;
    Ok(parse_algolia(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits_and_derives_domain() {
        let body = r#"{
          "hits": [
            {"objectID":"1","title":"A tool","url":"https://www.example.dev/a","points":10,"num_comments":3},
            {"objectID":"2","title":"Ask HN: something","points":5},
            {"objectID":"3","points":1}
          ]
        }"#;
        let items = parse_algolia(body);
        assert_eq!(items.len(), 2); // item 3 dropped: no title
        assert_eq!(items[0].hn_id, "1");
        assert_eq!(items[0].domain, "example.dev"); // www. stripped
        assert_eq!(items[1].url, "https://news.ycombinator.com/item?id=2"); // fallback url
        assert_eq!(items[1].domain, "news.ycombinator.com");
    }

    #[test]
    fn bad_json_yields_empty() {
        assert!(parse_algolia("not json").is_empty());
    }
}
