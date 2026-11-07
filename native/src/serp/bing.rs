// n2-kraken: Bing SERP parser
use super::super::error::KrakenError;
use super::super::types::SearchResult;
use scraper::{Html, Selector};
use std::sync::LazyLock;

static SEL_RESULT: LazyLock<Selector> = LazyLock::new(|| Selector::parse("li.b_algo").unwrap());
static SEL_LINK: LazyLock<Selector> = LazyLock::new(|| Selector::parse("h2 a").unwrap());
static SEL_SNIPPET: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("p, .b_caption p").unwrap());

/// Decode base64url (RFC 4648 section 5) without external dependency.
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for c in input.chars() {
        let v: u32 = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            _ => return None,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Bing wraps organic URLs in tracking redirects:
///   https://www.bing.com/ck/a?...&u=a1<base64url>&...
/// The `a1` prefix is Bing's marker; strip it and base64url-decode the rest.
/// Returns the original URL on any failure (caller still gets a usable link).
fn unwrap_bing_redirect(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_owned();
    };
    let host = parsed.host_str().unwrap_or_default();
    if !(host == "bing.com" || host.ends_with(".bing.com")) || parsed.path() != "/ck/a" {
        return url.to_owned();
    }
    for (name, value) in parsed.query_pairs() {
        if name != "u" {
            continue;
        }
        let encoded = value.strip_prefix("a1").unwrap_or(&value);
        let Some(bytes) = base64url_decode(encoded) else {
            continue;
        };
        let Ok(decoded) = String::from_utf8(bytes) else {
            continue;
        };
        if let Ok(destination) = crate::destination::parse(&decoded) {
            return destination.to_string();
        }
    }
    url.to_owned()
}

/// Parses a Bing SERP HTML string and returns organic search results.
pub fn parse_bing_serp(html_content: &str, limit: usize) -> Result<Vec<SearchResult>, KrakenError> {
    let document = Html::parse_document(html_content);
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for node in document.select(&SEL_RESULT) {
        if results.len() >= limit {
            break;
        }

        let (url, title) = match node.select(&SEL_LINK).next() {
            Some(a) => {
                let href = a.value().attr("href").unwrap_or("").to_string();
                let title = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if href.starts_with("http") && !title.is_empty() {
                    (unwrap_bing_redirect(&href), title)
                } else {
                    continue;
                }
            }
            None => continue,
        };

        let snippet = snippet(node);

        let Ok(url) = crate::destination::parse(&url) else {
            continue;
        };
        let url = url.to_string();
        if !seen.insert(url.clone()) {
            continue;
        }
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    Ok(results)
}

fn snippet(node: scraper::ElementRef<'_>) -> String {
    node.select(&SEL_SNIPPET)
        .next()
        .map(|el| {
            let s = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
            if s.len() > 300 {
                let boundary = s.floor_char_boundary(300);
                format!("{}...", &s[..boundary])
            } else {
                s
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_unwrap_is_scoped_to_actual_bing_hosts() {
        let unrelated = "https://notbing.com/ck/a?u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw";
        assert_eq!(unwrap_bing_redirect(unrelated), unrelated);
        let html = "<li class='b_algo'><h2><a href='https://user:secret@example.com/'>Invalid</a></h2></li><li class='b_algo'><h2><a href='https://example.com/page#one'>First</a></h2></li><li class='b_algo'><h2><a href='https://example.com/page#two'>Duplicate</a></h2></li>";
        let results = parse_bing_serp(html, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
    }

    #[test]
    fn test_base64url_decode_basic() {
        // "https://rust-lang.org/" base64url-encoded
        let encoded = "aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw";
        let decoded = base64url_decode(encoded).unwrap();
        assert_eq!(
            String::from_utf8(decoded).unwrap(),
            "https://rust-lang.org/"
        );
    }

    #[test]
    fn test_unwrap_bing_redirect_real() {
        let wrapped = "https://www.bing.com/ck/a?!&&p=xyz&u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw&ntb=1";
        assert_eq!(unwrap_bing_redirect(wrapped), "https://rust-lang.org/");
    }

    #[test]
    fn test_unwrap_bing_redirect_passthrough() {
        let direct = "https://example.com/page";
        assert_eq!(unwrap_bing_redirect(direct), direct);
    }

    #[test]
    fn test_unwrap_bing_redirect_malformed_returns_original() {
        let bad = "https://www.bing.com/ck/a?u=a1!!!invalid!!!";
        assert_eq!(unwrap_bing_redirect(bad), bad);
    }
}
