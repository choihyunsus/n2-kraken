// Engine-specific transports and bounded normalized public search-result records.
use super::serp::{bing, ddg, google, naver};
use super::{KrakenError, KrakenFetcher, SearchResult, StudyConfig};
use std::collections::HashSet;

pub(crate) async fn query(
    engine: &str,
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    let results = match engine {
        "searxng" => {
            let response =
                super::searxng::fetch(&config.searxng_url, query, &config.time_range).await?;
            parse_searxng(&response, config.max_results)?
        }
        "ddg" => search_ddg(query, config, fetcher).await?,
        "bing" => search_bing(query, config, fetcher).await?,
        "naver" => search_naver(query, config, fetcher).await?,
        "google" => search_google(query, config).await?,
        _ => {
            return Err(KrakenError::InvalidInput(
                "Unsupported search engine".into(),
            ));
        }
    };
    normalize(results, config.max_results)
}

async fn search_bing(
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    let url = format!(
        "https://www.bing.com/search?q={}&count={}",
        encode(query),
        config.max_results
    );
    let html = fetcher.fetch_html(&url).await?;
    checked_html(
        "bing",
        &html,
        bing::parse_bing_serp(&html, config.max_results)?,
    )
}

async fn search_naver(
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    let url = format!(
        "https://search.naver.com/search.naver?query={}",
        encode(query)
    );
    let html = fetcher.fetch_html(&url).await?;
    checked_html(
        "naver",
        &html,
        naver::parse_naver_serp(&html, config.max_results)?,
    )
}

async fn search_google(
    query: &str,
    config: &StudyConfig,
) -> Result<Vec<SearchResult>, KrakenError> {
    let browser = super::browser::HeadlessBrowser::global()
        .await
        .map_err(|_| KrakenError::Parse("Search browser unavailable".into()))?;
    let url = format!(
        "https://www.google.com/search?q={}&hl=en&gl=us&pws=0&udm=14&filter=0",
        encode(query)
    );
    let html = browser
        .fetch_rendered(&url)
        .await
        .map_err(|_| KrakenError::Parse("Rendered search failed".into()))?;
    google::parse_google_serp(&html, config.max_results)
}

fn encode(query: &str) -> String {
    url::form_urlencoded::byte_serialize(query.as_bytes()).collect()
}

async fn search_ddg(
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    let mut ranges = vec![config.time_range.as_str()];
    for range in ["3m", "6m", "1y", ""] {
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    for range in ranges {
        let filter = match range {
            "1w" => "w",
            "1m" => "m",
            "3m" => "m3",
            "6m" => "m6",
            "1y" => "y",
            _ => "",
        };
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}&df={filter}",
            encode(query)
        );
        let html = fetcher.fetch_html(&url).await?;
        let results = checked_html("ddg", &html, ddg::parse_ddg_serp(&html, config.max_results))?;
        if !results.is_empty() {
            return Ok(results);
        }
    }
    Ok(Vec::new())
}

fn checked_html(
    engine: &str,
    html: &str,
    results: Vec<SearchResult>,
) -> Result<Vec<SearchResult>, KrakenError> {
    if !results.is_empty() {
        return Ok(results);
    }
    let markers: &[&str] = match engine {
        "ddg" => &["no results found", "no more results"],
        "bing" => &[
            "there are no results for",
            "we could not find any results for",
        ],
        "naver" => &["검색결과가 없습니다", "검색 결과가 없습니다"],
        _ => &[],
    };
    let text = html.to_lowercase();
    if markers.iter().any(|marker| text.contains(marker)) {
        return Ok(results);
    }
    Err(KrakenError::Parse(
        "Search response has unknown, challenged or consent-only markup".into(),
    ))
}

#[derive(serde::Deserialize)]
struct SearxngResponse {
    results: Vec<SearxngResult>,
}
#[derive(serde::Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
}

pub(crate) fn parse_searxng(input: &str, limit: usize) -> Result<Vec<SearchResult>, KrakenError> {
    let response: SearxngResponse = serde_json::from_str(input)
        .map_err(|_| KrakenError::Parse("Invalid SearXNG response".into()))?;
    normalize(
        response
            .results
            .into_iter()
            .take(200)
            .map(|result| SearchResult {
                title: result.title,
                url: result.url,
                snippet: result.content,
            })
            .collect(),
        limit,
    )
}

fn normalize(results: Vec<SearchResult>, limit: usize) -> Result<Vec<SearchResult>, KrakenError> {
    let offered = !results.is_empty();
    let mut seen = HashSet::new();
    let mut checked = Vec::new();
    for item in results.into_iter().take(200) {
        if checked.len() >= limit {
            break;
        }
        let Ok(url) = super::destination::parse(&item.url) else {
            continue;
        };
        let url = url.to_string();
        if item.title.trim().is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        checked.push(SearchResult {
            url,
            title: preview(item.title, 1024),
            snippet: preview(item.snippet, 4096),
        });
    }
    if offered && limit > 0 && checked.is_empty() {
        return Err(KrakenError::Parse(
            "Search response contains no usable HTTP(S) results".into(),
        ));
    }
    Ok(checked)
}

fn preview(value: String, limit: usize) -> String {
    let text = value.trim();
    text[..text.floor_char_boundary(limit.min(text.len()))].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenged_markup_is_separate_from_a_recognized_empty_search() {
        for engine in ["ddg", "bing", "naver"] {
            assert!(checked_html(engine, "<html>Confirm you are human</html>", vec![]).is_err());
        }
        assert!(
            checked_html("ddg", "No results found", vec![])
                .unwrap()
                .is_empty()
        );
        assert!(
            checked_html("naver", "검색결과가 없습니다", vec![])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn result_normalization_preserves_unicode_deduplicates_and_rejects_active_urls() {
        let item = |url: &str| SearchResult {
            title: "한글😀".repeat(1000),
            url: url.into(),
            snippet: "가".repeat(5000),
        };
        let results = normalize(
            vec![
                item("https://example.com/path#one"),
                item("https://example.com/path#two"),
                item("javascript:alert(1)"),
                item("https://user:secret@example.com/"),
            ],
            10,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/path");
        assert!(results[0].title.len() <= 1024 && results[0].snippet.len() <= 4096);
        assert!(normalize(vec![item("file:///private")], 10).is_err());
    }
}
