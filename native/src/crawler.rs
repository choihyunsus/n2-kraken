// Bounded HTTP-first crawling retains exact source identity, content hashes and per-page failures.
use super::browser::HeadlessBrowser;
use super::config::StudyConfig;
use super::error::KrakenError;
use super::fetcher::KrakenFetcher;
use super::html::{HtmlToMarkdownConfig, parse_html_to_markdown};
use super::types::{CrawlFailure, CrawlReport, CrawledPage};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MIN_CONTENT_LEN: usize = 50;
const MAX_TITLE_BYTES: usize = 8192;

pub async fn crawl_pages(
    urls: &[String],
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<CrawlReport, KrakenError> {
    config.validate()?;
    tokio::time::timeout(
        std::time::Duration::from_millis(config.total_timeout_ms),
        crawl_inner(urls, config, fetcher),
    )
    .await
    .map_err(|_| KrakenError::Timeout(config.total_timeout_ms))?
}

async fn crawl_inner(
    urls: &[String],
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<CrawlReport, KrakenError> {
    if urls.len() > 40 {
        return Err(KrakenError::InvalidInput(
            "At most 40 crawl URLs are supported".into(),
        ));
    }
    let mut seen = HashSet::new();
    let urls = urls
        .iter()
        .map(|url| {
            crate::destination::parse(url)
                .map(|url| url.to_string())
                .map_err(KrakenError::InvalidInput)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut report = CrawlReport::default();
    let mut retry = Vec::new();
    for url in urls {
        if report.pages.len() >= config.max_crawl_pages {
            break;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        match crawl_raw(&url, config, fetcher).await {
            RawResult::Page(page) => retain_page(&mut report, page),
            RawResult::Short(final_url) => retry.push(final_url),
            RawResult::Failed(reason) => report.failures.push(failure(&url, "http", &reason)),
        }
    }
    render_short_pages(&retry, config, &mut report).await;
    Ok(report)
}

enum RawResult {
    Page(CrawledPage),
    Short(String),
    Failed(String),
}

async fn crawl_raw(url: &str, config: &StudyConfig, fetcher: &KrakenFetcher) -> RawResult {
    let request = tokio::time::timeout(
        std::time::Duration::from_millis(config.timeout_ms),
        fetcher.fetch_document(url),
    )
    .await;
    let document = match request {
        Ok(Ok(document)) => document,
        Ok(Err(error)) => return RawResult::Failed(error.to_string()),
        Err(_) => return RawResult::Failed("HTTP page deadline exceeded".into()),
    };
    let markdown = match parse_html_to_markdown(
        &document.html,
        &HtmlToMarkdownConfig {
            include_links: false,
            include_images: false,
            max_length: config.max_content_length,
        },
    ) {
        Ok(document) => document,
        Err(error) => return RawResult::Failed(error.to_string()),
    };
    if markdown.markdown.len() < MIN_CONTENT_LEN {
        return RawResult::Short(document.url);
    }
    RawResult::Page(page(
        document.url,
        markdown.title,
        markdown.markdown,
        markdown.truncated,
    ))
}

async fn render_short_pages(urls: &[String], config: &StudyConfig, report: &mut CrawlReport) {
    if urls.is_empty() || report.pages.len() >= config.max_crawl_pages {
        return;
    }
    let browser = match tokio::time::timeout(
        std::time::Duration::from_millis(config.timeout_ms),
        HeadlessBrowser::global(),
    )
    .await
    {
        Ok(Ok(browser)) => browser,
        _ => {
            for url in urls {
                report
                    .failures
                    .push(failure(url, "render", "Browser initialization unavailable"));
            }
            return;
        }
    };
    let mut seen = HashSet::new();
    for url in urls {
        if report.pages.len() >= config.max_crawl_pages {
            break;
        }
        if !seen.insert(url) {
            continue;
        }
        match crawl_rendered(url, config, browser).await {
            Ok(page) => retain_page(report, page),
            Err(reason) => report.failures.push(failure(url, "render", &reason)),
        }
    }
}

async fn crawl_rendered(
    url: &str,
    config: &StudyConfig,
    browser: &HeadlessBrowser,
) -> Result<CrawledPage, String> {
    let cleaned = tokio::time::timeout(
        std::time::Duration::from_millis(config.timeout_ms),
        browser.fetch_cleaned(url),
    )
    .await
    .map_err(|_| "Rendered page deadline exceeded")?
    .map_err(|_| "Rendered page retrieval failed")?;
    let url = crate::destination::parse(&cleaned.url)?.to_string();
    let truncated = cleaned.truncated || cleaned.content.len() > config.max_content_length;
    let content = truncate(cleaned.content, config.max_content_length);
    if content.len() < MIN_CONTENT_LEN {
        return Err("Rendered page has insufficient content".into());
    }
    Ok(page(url, cleaned.title, content, truncated))
}

fn page(url: String, title: String, content: String, truncated: bool) -> CrawledPage {
    let content_hash = Sha256::digest(content.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    CrawledPage {
        url,
        truncated: truncated || title.len() > MAX_TITLE_BYTES,
        title: truncate(title, MAX_TITLE_BYTES),
        content_length: content.len(),
        content_hash,
        content,
        fetched_at,
    }
}

fn retain_page(report: &mut CrawlReport, page: CrawledPage) {
    if report.pages.iter().any(|previous| previous.url == page.url) {
        report
            .failures
            .push(failure(&page.url, "identity", "Duplicate final source URL"));
    } else {
        report.pages.push(page);
    }
}

fn truncate(mut text: String, maximum: usize) -> String {
    text.truncate(text.floor_char_boundary(maximum.min(text.len())));
    text
}

fn failure(url: &str, stage: &str, reason: &str) -> CrawlFailure {
    CrawlFailure {
        url: url.to_owned(),
        stage: stage.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_receipts_bind_exact_content_and_deduplicate_final_urls() {
        let first = page(
            "https://example.com/final".into(),
            "한글".repeat(5000),
            "hello".into(),
            false,
        );
        assert_eq!(
            first.content_hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(first.content_length, 5);
        assert!(first.title.len() <= MAX_TITLE_BYTES);
        assert!(first.truncated);
        let mut report = CrawlReport::default();
        retain_page(&mut report, first.clone());
        retain_page(&mut report, first);
        assert_eq!(report.pages.len(), 1);
        assert_eq!(report.failures[0].stage, "identity");
    }

    #[tokio::test]
    async fn blocked_pages_have_failure_evidence_and_never_open_the_target() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/private", listener.local_addr().unwrap());
        let fetcher = KrakenFetcher::new();
        let config = StudyConfig::default();
        let report = crawl_pages(std::slice::from_ref(&url), &config, &fetcher)
            .await
            .unwrap();
        assert!(report.pages.is_empty());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].url, url);
        assert_eq!(report.failures[0].stage, "http");
        assert!(
            crawl_pages(&[url, "file:///private".into()], &config, &fetcher)
                .await
                .is_err()
        );
        assert!(
            crawl_pages(&vec!["https://example.com/".into(); 41], &config, &fetcher)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), listener.accept())
                .await
                .is_err()
        );
    }
}
