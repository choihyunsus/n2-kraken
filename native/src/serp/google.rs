// n2-kraken: Google SERP parser — multi-selector fallback strategy
use super::super::error::KrakenError;
use super::super::types::SearchResult;
use scraper::{ElementRef, Html, Selector};
use std::sync::LazyLock;

// Primary: classic div.g containers
static SEL_G: LazyLock<Selector> = LazyLock::new(|| Selector::parse("div.g").unwrap());
// Fallback: #rso (Results Section Organic) children with data-hveid
static SEL_RSO: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("#rso div[data-hveid][lang]").unwrap());
// Fallback 2: MjjYud wrapper (2024-2026 structure)
static SEL_MJJYUD: LazyLock<Selector> = LazyLock::new(|| Selector::parse("#rso .MjjYud").unwrap());
static SEL_A: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").unwrap());
static SEL_H3: LazyLock<Selector> = LazyLock::new(|| Selector::parse("h3").unwrap());
static SEL_CHALLENGE: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(".g-recaptcha").unwrap());
// Snippet selectors (most specific first)
static SEL_SNIPPET_VWIC3B: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(".VwiC3b").unwrap());
static SEL_SNIPPET_SNCF: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("[data-sncf=\"1\"]").unwrap());

/// Parses a Google SERP HTML string and returns top N organic results, skipping ads.
/// Uses multi-selector fallback: div.g → #rso div[data-hveid] → .MjjYud
pub fn parse_google_serp(
    html_content: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, KrakenError> {
    let document = Html::parse_document(html_content);

    if limit == 0 {
        return Ok(Vec::new());
    }
    if document.select(&SEL_CHALLENGE).next().is_some() {
        return Err(KrakenError::Parse(
            "Google requires interactive browser verification".into(),
        ));
    }
    for selector in [&*SEL_G, &*SEL_RSO, &*SEL_MJJYUD] {
        let results = parse_nodes(document.select(selector), limit);
        if !results.is_empty() {
            return Ok(results);
        }
    }
    if html_content.contains("did not match any documents") {
        return Ok(Vec::new());
    }
    Err(KrakenError::Parse(
        "Google did not provide recognizable results (challenge, consent, or changed markup)"
            .into(),
    ))
}

fn parse_nodes<'a>(nodes: impl Iterator<Item = ElementRef<'a>>, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for node in nodes.take(200) {
        if results.len() >= limit {
            break;
        }
        let Some((url, title)) = extract_link_title(&node) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let snippet = extract_snippet(&node, &title);
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
    results
}

/// Extract first valid (link, title) pair from a result container.
fn extract_link_title(node: &ElementRef) -> Option<(String, String)> {
    for a_node in node.select(&SEL_A) {
        if let Some(h3_node) = a_node.select(&SEL_H3).next()
            && let Some(href) = a_node.value().attr("href")
            && let Some(url) = organic_link(href)
        {
            let title = h3_node.text().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                return Some((url, title));
            }
        }
    }
    None
}

/// Check if a link is an organic result (not ad, not internal Google link).
fn organic_link(href: &str) -> Option<String> {
    let url = crate::destination::parse(href).ok()?;
    let host = url.host_str()?;
    let google = host == "google.com" || host.ends_with(".google.com");
    if host == "googleadservices.com"
        || host.ends_with(".googleadservices.com")
        || (google && matches!(url.path(), "/aclk" | "/search"))
    {
        return None;
    }
    Some(url.to_string())
}

/// Extract snippet text from a result container.
/// Tries dedicated snippet selectors first, falls back to text collection.
fn extract_snippet(node: &ElementRef, title: &str) -> String {
    // Strategy 1: .VwiC3b (standard snippet class)
    if let Some(el) = node.select(&SEL_SNIPPET_VWIC3B).next() {
        let text: String = el.text().collect::<Vec<_>>().join(" ");
        let trimmed = text.trim().to_string();
        if trimmed.len() > 20 {
            return truncate_snippet(trimmed);
        }
    }

    // Strategy 2: [data-sncf="1"] (data attribute, more stable)
    if let Some(el) = node.select(&SEL_SNIPPET_SNCF).next() {
        let text: String = el.text().collect::<Vec<_>>().join(" ");
        let trimmed = text.trim().to_string();
        if trimmed.len() > 20 {
            return truncate_snippet(trimmed);
        }
    }

    // Strategy 3: collect all text excluding title (original fallback)
    let mut snippet = String::new();
    for text_node in node.text() {
        let t = text_node.trim();
        if !t.is_empty() && !title.contains(t) && t.len() > 2 {
            if !snippet.is_empty() {
                snippet.push(' ');
            }
            snippet.push_str(t);
        }
    }
    truncate_snippet(snippet)
}

fn truncate_snippet(mut s: String) -> String {
    if s.len() > 300 {
        s.truncate(s.floor_char_boundary(300));
        s.push_str("...");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_verification_is_an_explicit_search_failure() {
        let result = parse_google_serp("<main><div class='g-recaptcha'></div></main>", 5);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("interactive browser verification")
        );
    }

    #[test]
    fn fallback_is_tried_when_primary_containers_have_no_usable_links() {
        let html = "<div class='g'><a href='https://www.googleadservices.com/ad'><h3>Advertisement</h3></a></div><div id='rso'><div data-hveid='fixture' lang='en'><a href='https://example.com/actual#heading'><h3>Actual title</h3></a></div></div>";
        let results = parse_google_serp(html, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/actual");
        assert!(parse_google_serp("Unknown markup", 0).unwrap().is_empty());
    }

    #[test]
    fn test_parse_classic_div_g() {
        let html = r#"<html><body>
        <div id="rso">
            <div class="g">
                <a href="https://example.com/page1">
                    <h3>Example Page Title</h3>
                </a>
                <div class="VwiC3b">This is a snippet describing the search result content.</div>
            </div>
            <div class="g">
                <a href="https://example.com/page2">
                    <h3>Second Result Title</h3>
                </a>
                <div class="VwiC3b">Another snippet for the second search result here.</div>
            </div>
        </div>
        </body></html>"#;

        let results = parse_google_serp(html, 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Page Title");
        assert_eq!(results[0].url, "https://example.com/page1");
        assert!(results[0].snippet.contains("snippet describing"));
        assert_eq!(results[1].title, "Second Result Title");
    }

    #[test]
    fn test_fallback_rso_data_hveid() {
        let html = r#"<html><body>
        <div id="rso">
            <div data-hveid="CAQ" lang="en">
                <a href="https://fallback.com/result">
                    <h3>Fallback Result</h3>
                </a>
                <div data-sncf="1">Fallback snippet via data attribute selector.</div>
            </div>
        </div>
        </body></html>"#;

        let results = parse_google_serp(html, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Fallback Result");
        assert!(results[0].snippet.contains("Fallback snippet"));
    }

    #[test]
    fn test_skips_ads() {
        let html = r#"<html><body>
        <div id="rso">
            <div class="g">
                <a href="https://www.googleadservices.com/pagead/aclk?sa=l">
                    <h3>Ad Result</h3>
                </a>
            </div>
            <div class="g">
                <a href="https://organic.com/real">
                    <h3>Real Organic Result</h3>
                </a>
                <div class="VwiC3b">Real content here for organic result.</div>
            </div>
        </div>
        </body></html>"#;

        let results = parse_google_serp(html, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://organic.com/real");
    }

    #[test]
    fn test_deduplicates_urls() {
        let html = r#"<html><body>
        <div id="rso">
            <div class="g">
                <a href="https://example.com/same"><h3>First</h3></a>
                <div class="VwiC3b">First occurrence snippet text here.</div>
            </div>
            <div class="g">
                <a href="https://example.com/same"><h3>Duplicate</h3></a>
                <div class="VwiC3b">Duplicate occurrence snippet text here.</div>
            </div>
        </div>
        </body></html>"#;

        let results = parse_google_serp(html, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "First");
    }

    #[test]
    fn test_respects_limit() {
        let html = r#"<html><body><div id="rso">
            <div class="g"><a href="https://a.com"><h3>A</h3></a><div class="VwiC3b">Snippet A content text.</div></div>
            <div class="g"><a href="https://b.com"><h3>B</h3></a><div class="VwiC3b">Snippet B content text.</div></div>
            <div class="g"><a href="https://c.com"><h3>C</h3></a><div class="VwiC3b">Snippet C content text.</div></div>
        </div></body></html>"#;

        let results = parse_google_serp(html, 2).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_unknown_markup_is_not_a_successful_empty_search() {
        let html = "<html><body><div>No results</div></body></html>";
        assert!(parse_google_serp(html, 10).is_err());
        assert!(
            parse_google_serp("did not match any documents", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_snippet_truncation() {
        let long_text = "A".repeat(400);
        let html = format!(
            r#"<html><body><div id="rso">
            <div class="g">
                <a href="https://long.com"><h3>Long</h3></a>
                <div class="VwiC3b">{}</div>
            </div>
        </div></body></html>"#,
            long_text
        );

        let results = parse_google_serp(&html, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].snippet.len() <= 303); // 300 + "..."
        assert!(results[0].snippet.ends_with("..."));
    }
}
