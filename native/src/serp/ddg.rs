// n2-kraken: DuckDuckGo result extraction using the HTML tree.
use super::super::types::SearchResult;
use scraper::{ElementRef, Html, Selector};
use std::collections::HashSet;
use std::sync::LazyLock;

static LINKS: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a.result__a").unwrap());
static SNIPPET: LazyLock<Selector> = LazyLock::new(|| Selector::parse(".result__snippet").unwrap());

fn text(node: ElementRef<'_>) -> String {
    node.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn actual_url(raw: &str) -> Option<String> {
    let absolute = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let mut url = url::Url::parse(&absolute).ok()?;
    if matches!(
        url.host_str(),
        Some("duckduckgo.com" | "html.duckduckgo.com")
    ) && url.path() == "/l/"
    {
        let destination = url
            .query_pairs()
            .find(|(name, _)| name == "uddg")?
            .1
            .into_owned();
        url = url::Url::parse(&destination).ok()?;
    }
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn snippet_for(link: ElementRef<'_>) -> String {
    for parent in link.ancestors().take(6).filter_map(ElementRef::wrap) {
        if matches!(parent.value().name(), "body" | "html") {
            break;
        }
        // Do not pair one result's title with another result's description.
        if parent.select(&LINKS).take(2).count() > 1 {
            break;
        }
        if let Some(description) = parent.select(&SNIPPET).next() {
            return text(description);
        }
    }
    String::new()
}

/// Preserve nested emphasis, HTML entities and title/description associations.
pub fn parse_ddg_serp(html: &str, max_results: usize) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for link in document.select(&LINKS) {
        if results.len() >= max_results {
            break;
        }
        let Some(url) = actual_url(link.value().attr("href").unwrap_or("")) else {
            continue;
        };
        let title = text(link);
        if title.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        results.push(SearchResult {
            title,
            url,
            snippet: snippet_for(link),
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_emphasis_entities_and_attribute_order_preserve_complete_text() {
        let html = r#"<div class='result__body links_main'>
          <h2><a href='https://example.com/book?a=1&amp;b=2' class='extra result__a'><b>Rust</b> &amp; SQLite</a></h2>
          <div class='result__snippet'>A <b>Rust</b> language &amp; database guide.</div>
        </div>"#;
        let result = parse_ddg_serp(html, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Rust & SQLite");
        assert_eq!(result[0].snippet, "A Rust language & database guide.");
        assert_eq!(result[0].url, "https://example.com/book?a=1&b=2");
    }

    #[test]
    fn missing_snippet_cannot_shift_the_following_result_description() {
        let html = r#"<section>
          <div class='result'><a class='result__a' href='https://example.com/first'>First</a></div>
          <div class='result'><a class='result__a' href='https://example.com/second'>Second</a><a class='result__snippet'>Second description</a></div>
        </section>"#;
        let result = parse_ddg_serp(html, 5);
        assert_eq!(result.len(), 2);
        assert!(result[0].snippet.is_empty());
        assert_eq!(result[1].snippet, "Second description");
    }

    #[test]
    fn redirect_unicode_limit_and_unsafe_destination_handling() {
        let html = r#"<div class='result'>
          <a class='result__a' href='//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F%ED%95%9C%EA%B8%80&amp;rut=abc'>한글</a>
          <a class='result__a' href='https://example.com/%ED%95%9C%EA%B8%80#duplicate'>Duplicate</a>
          <a class='result__a' href='//duckduckgo.com/l/?uddg=javascript%3Aalert(1)'>Unsafe</a>
          <a class='result__a' href='https://name:secret@example.com/'>Credential URL</a>
          <a class='result__a' href='https://example.com/?uddg=keep'>Unrelated query</a>
        </div>"#;
        let result = parse_ddg_serp(html, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].url, "https://example.com/%ED%95%9C%EA%B8%80");
        assert_eq!(result[1].url, "https://example.com/?uddg=keep");
        assert!(parse_ddg_serp(html, 0).is_empty());
        assert_eq!(parse_ddg_serp(html, 1).len(), 1);
    }
}
