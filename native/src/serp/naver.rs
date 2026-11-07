// n2-kraken: Naver organic result titles, not every outbound navigation link.
use super::super::{error::KrakenError, types::SearchResult};
use scraper::{ElementRef, Html, Selector};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static LINKS: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").unwrap());
static HEADLINE: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse(".sds-comps-text-type-headline1, .sds-comps-text-type-headline2").unwrap()
});
static DESCRIPTION: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse(".sds-comps-text-type-body1, .total_dsc, .dsc_txt, .news_dsc").unwrap()
});

fn text(node: ElementRef<'_>) -> String {
    node.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn result_url(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    // Naver content is valid. Search and ad navigation endpoints are not.
    if matches!(
        url.host_str()?,
        "search.naver.com" | "adcr.naver.com" | "ader.naver.com"
    ) {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn title(link: ElementRef<'_>) -> Option<String> {
    if let Some(headline) = link.select(&HEADLINE).next() {
        return Some(text(headline));
    }
    let legacy = link
        .value()
        .classes()
        .any(|class| matches!(class, "link_tit" | "title_link" | "total_tit" | "news_tit"));
    legacy.then(|| text(link).trim_end_matches("새 창 열림").trim().to_string())
}

fn legacy_snippet(link: ElementRef<'_>) -> String {
    // Keep extraction inside the nearby result container, never the page body.
    link.ancestors()
        .take(4)
        .filter_map(ElementRef::wrap)
        .take_while(|parent| !matches!(parent.value().name(), "body" | "html"))
        .find_map(|parent| parent.select(&DESCRIPTION).next().map(text))
        .unwrap_or_default()
}

/// Parse headline anchors and associate descriptions by the actual result URL.
pub fn parse_naver_serp(
    html_content: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, KrakenError> {
    let document = Html::parse_document(html_content);
    let links: Vec<_> = document.select(&LINKS).collect();
    let mut descriptions = HashMap::new();
    for link in &links {
        if let (Some(url), Some(description)) = (
            result_url(link.value().attr("href").unwrap_or("")),
            link.select(&DESCRIPTION).next(),
        ) {
            descriptions.entry(url).or_insert_with(|| text(description));
        }
    }
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for link in links {
        if results.len() >= limit {
            break;
        }
        let Some(url) = result_url(link.value().attr("href").unwrap_or("")) else {
            continue;
        };
        let Some(title) = title(link).filter(|value| !value.is_empty()) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let snippet = descriptions
            .get(&url)
            .cloned()
            .unwrap_or_else(|| legacy_snippet(link));
        let snippet = if snippet.chars().count() > 300 {
            format!("{}...", snippet.chars().take(300).collect::<String>())
        } else {
            snippet
        };
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_headline_ignores_favicon_breadcrumb_and_accessibility_label() {
        let html = r#"
          <a target="_blank" href="https://example.com/">새 창 열림</a>
          <a target="_blank" href="https://example.com/rust">example.com › Rust 새 창 열림</a>
          <a href="https://example.com/rust"><span class="sds-comps-text-type-headline1"><mark>Rust</mark> 언어</span><span>새 창 열림</span></a>
          <a href="https://example.com/rust"><span class="sds-comps-text-type-body1">한글 <mark>설명</mark>입니다.</span><span>새 창 열림</span></a>
          <a href="https://example.com/rust#history">역사 새 창 열림</a>
          <a href="https://example.com/rust#repeat"><span class="sds-comps-text-type-headline1">중복</span></a>
        "#;
        let results = parse_naver_serp(html, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust 언어");
        assert_eq!(results[0].url, "https://example.com/rust");
        assert_eq!(results[0].snippet, "한글 설명입니다.");
    }

    #[test]
    fn legacy_and_naver_content_are_retained_but_navigation_and_unsafe_urls_are_not() {
        let html = r#"
          <a class="link_tit" href="javascript:alert(1)">Unsafe</a>
          <a class="news_tit" href="https://search.naver.com/search.naver?q=more">More</a>
          <a class="link_tit" href="https://name:secret@example.com/">Credentials</a>
          <div><a class="title_link" href="https://blog.naver.com/writer/1">짧음</a><p class="dsc_txt">블로그 설명</p></div>
          <a class="news_tit" href="https://news.naver.com/article/1">뉴스 제목</a>
        "#;
        let results = parse_naver_serp(html, 5).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "짧음");
        assert_eq!(results[0].snippet, "블로그 설명");
        assert!(results[1].url.starts_with("https://news.naver.com/"));
        assert!(results[1].snippet.is_empty());
        assert!(parse_naver_serp(html, 0).unwrap().is_empty());
        assert_eq!(parse_naver_serp(html, 1).unwrap().len(), 1);
    }

    #[test]
    fn malformed_html_and_unicode_snippet_do_not_panic() {
        let html = format!(
            "<a href='https://example.com/'><span class='sds-comps-text-type-headline1'>제목</span></a><a href='https://example.com/'><span class='sds-comps-text-type-body1'>{}",
            "가😀".repeat(200)
        );
        let result = parse_naver_serp(&html, 1).unwrap();
        assert_eq!(result[0].snippet.chars().count(), 303);
        assert!(result[0].snippet.ends_with("..."));
    }
}
