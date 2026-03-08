use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const BRAVE_WEB_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const BRAVE_NEWS_URL: &str = "https://api.search.brave.com/res/v1/news/search";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveWebResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub extra_snippets: Vec<String>,
    pub age: Option<String>,
    pub page_age: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveNewsResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub source: Option<String>,
    pub age: Option<String>,
    pub page_age: Option<String>,
    pub extra_snippets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BraveWebSearchResponse {
    web: BraveWebPayload,
}

#[derive(Debug, Deserialize)]
struct BraveWebPayload {
    results: Vec<BraveWebItem>,
}

#[derive(Debug, Deserialize)]
struct BraveWebItem {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    extra_snippets: Vec<String>,
    age: Option<String>,
    page_age: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BraveNewsSearchResponse {
    results: Vec<BraveNewsItem>,
}

#[derive(Debug, Deserialize)]
struct BraveNewsItem {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    extra_snippets: Vec<String>,
    age: Option<String>,
    page_age: Option<String>,
    source: Option<String>,
    meta_url: Option<BraveMetaUrl>,
}

#[derive(Debug, Deserialize)]
struct BraveMetaUrl {
    hostname: Option<String>,
}

pub async fn brave_web_search(
    key: &str,
    query: &str,
    freshness: Option<&str>,
    count: usize,
) -> Result<Vec<BraveWebResult>> {
    ensure_key(key)?;
    let body = request(key, BRAVE_WEB_URL, query, freshness, count).await?;
    let parsed: BraveWebSearchResponse =
        serde_json::from_str(&body).context("Failed to parse Brave web search response")?;
    let results = parsed
        .web
        .results
        .into_iter()
        .map(|r| BraveWebResult {
            title: r.title,
            url: r.url,
            description: r.description,
            extra_snippets: r.extra_snippets,
            age: r.age,
            page_age: r.page_age,
        })
        .collect();
    Ok(results)
}

pub async fn brave_news_search(
    key: &str,
    query: &str,
    freshness: Option<&str>,
    count: usize,
) -> Result<Vec<BraveNewsResult>> {
    ensure_key(key)?;
    let body = request(key, BRAVE_NEWS_URL, query, freshness, count).await?;
    let parsed: BraveNewsSearchResponse =
        serde_json::from_str(&body).context("Failed to parse Brave news search response")?;
    let results = parsed
        .results
        .into_iter()
        .map(|r| BraveNewsResult {
            title: r.title,
            url: r.url,
            description: r.description,
            source: r.source.or_else(|| r.meta_url.and_then(|m| m.hostname)),
            age: r.age,
            page_age: r.page_age,
            extra_snippets: r.extra_snippets,
        })
        .collect();
    Ok(results)
}

async fn request(
    key: &str,
    endpoint: &str,
    query: &str,
    freshness: Option<&str>,
    count: usize,
) -> Result<String> {
    if query.trim().is_empty() {
        bail!("Brave query cannot be empty");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .context("Failed to create Brave HTTP client")?;

    let count_string = count.clamp(1, 50).to_string();
    let mut req = client
        .get(endpoint)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .query(&[("q", query), ("count", count_string.as_str())]);

    if let Some(f) = freshness {
        req = req.query(&[("freshness", f)]);
    }

    let resp = req.send().await.context("Brave request failed")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    match status.as_u16() {
        200..=299 => Ok(text),
        401 => bail!("Brave API unauthorized (401). Check brave_api_key."),
        429 => bail!("Brave API rate limited (429). Try again later."),
        _ => bail!("Brave API error {}: {}", status, truncate(&text, 300)),
    }
}

fn ensure_key(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        bail!("Brave API key not configured")
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_key_errors() {
        let err = ensure_key("").unwrap_err().to_string();
        assert!(err.contains("not configured"));
    }
}
