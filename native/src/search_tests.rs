// Search API fixtures and explicitly requested live probes.
const SNIPPET_PREVIEW_CHARS: usize = 80;
use super::*;

#[test]
fn test_searxng_json_parse() {
    let json = r#"{
            "results": [
                {"title": "Rust Lang", "url": "https://rust-lang.org", "content": "A systems programming language."},
                {"title": "Rust Book", "url": "https://doc.rust-lang.org/book/", "content": "The official Rust tutorial."},
                {"title": "", "url": "", "content": "Empty entry"},
                {"title": "No URL", "url": "", "content": "Missing URL entry"}
            ]
        }"#;

    let results = crate::search_engines::parse_searxng(json, 10).unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Rust Lang");
    assert_eq!(results[0].url, "https://rust-lang.org/");
    assert_eq!(results[1].title, "Rust Book");
}

/// Live SearXNG test (requires SearXNG running, SEARXNG_URL env).
/// Run with: cargo test test_searxng_live -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_searxng_live() {
    tracing_subscriber::fmt()
        .with_env_filter("n2_kraken=debug")
        .try_init()
        .ok();

    let config = StudyConfig {
        max_results: 5,
        engines: vec!["searxng".to_string()],
        ..Default::default()
    };
    let fetcher = KrakenFetcher::new();

    let results = search("Rust programming language", &config, &fetcher).await;
    match results {
        Ok(ref r) => {
            println!("\n=== SEARXNG LIVE TEST ===");
            println!("Results: {}", r.len());
            for (i, sr) in r.iter().enumerate() {
                println!(
                    "[{}] {} | {} | {}",
                    i + 1,
                    sr.title,
                    sr.url,
                    sr.snippet
                        .chars()
                        .take(SNIPPET_PREVIEW_CHARS)
                        .collect::<String>()
                );
            }
            assert!(!r.is_empty(), "SearXNG returned 0 results");
        }
        Err(e) => {
            panic!("SearXNG search failed: {}", e);
        }
    }
}

/// Live DDG search test.
/// Run with: cargo test test_ddg_live -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_ddg_live() {
    tracing_subscriber::fmt()
        .with_env_filter("n2_kraken=debug")
        .try_init()
        .ok();

    let config = StudyConfig {
        max_results: 5,
        engines: vec!["ddg".to_string()],
        ..Default::default()
    };
    let fetcher = KrakenFetcher::new();

    let results = search("Rust programming language", &config, &fetcher).await;
    match results {
        Ok(ref r) => {
            println!("\n=== DDG LIVE TEST ===");
            println!("Results: {}", r.len());
            for (i, sr) in r.iter().enumerate() {
                let snip: String = sr.snippet.chars().take(SNIPPET_PREVIEW_CHARS).collect();
                println!("[{}] {} | {} | {}", i + 1, sr.title, sr.url, snip);
            }
            assert!(!r.is_empty(), "DDG returned 0 results");
        }
        Err(e) => panic!("DDG search failed: {}", e),
    }
}

/// Live Bing search test.
/// Run with: cargo test test_bing_live -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_bing_live() {
    tracing_subscriber::fmt()
        .with_env_filter("n2_kraken=debug")
        .try_init()
        .ok();

    let config = StudyConfig {
        max_results: 5,
        engines: vec!["bing".to_string()],
        ..Default::default()
    };
    let fetcher = KrakenFetcher::new();

    let results = search("Rust programming language", &config, &fetcher).await;
    match results {
        Ok(ref r) => {
            println!("\n=== BING LIVE TEST ===");
            println!("Results: {}", r.len());
            for (i, sr) in r.iter().enumerate() {
                let snip: String = sr.snippet.chars().take(SNIPPET_PREVIEW_CHARS).collect();
                println!("[{}] {} | {} | {}", i + 1, sr.title, sr.url, snip);
            }
            assert!(!r.is_empty(), "Bing returned 0 results");
        }
        Err(e) => panic!("Bing search failed: {}", e),
    }
}

/// Live Naver search test (Korean query).
/// Run with: cargo test test_naver_live -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_naver_live() {
    tracing_subscriber::fmt()
        .with_env_filter("n2_kraken=debug")
        .try_init()
        .ok();

    let config = StudyConfig {
        max_results: 5,
        engines: vec!["naver".to_string()],
        ..Default::default()
    };
    let fetcher = KrakenFetcher::new();

    let results = search("러스트 프로그래밍 언어", &config, &fetcher).await;
    match results {
        Ok(ref r) => {
            println!("\n=== NAVER LIVE TEST ===");
            println!("Results: {}", r.len());
            for (i, sr) in r.iter().enumerate() {
                let snip: String = sr.snippet.chars().take(SNIPPET_PREVIEW_CHARS).collect();
                println!("[{}] {} | {} | {}", i + 1, sr.title, sr.url, snip);
            }
            assert!(!r.is_empty(), "Naver returned 0 results");
        }
        Err(e) => panic!("Naver search failed: {}", e),
    }
}

/// Live Google search test (requires Chrome installed).
/// Run with: cargo test test_google_live -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn test_google_live() {
    tracing_subscriber::fmt()
        .with_env_filter("n2_kraken=debug")
        .try_init()
        .ok();

    let config = StudyConfig {
        max_results: 5,
        engines: vec!["google".to_string()],
        ..Default::default()
    };
    let fetcher = KrakenFetcher::new();

    let results = search("Rust programming language", &config, &fetcher).await;
    if results.is_err() {
        google_diagnostic().await;
    }
    crate::browser::shutdown().await;
    match results {
        Ok(ref r) => {
            println!("\n=== GOOGLE LIVE TEST ===");
            println!("Results: {}", r.len());
            for (i, sr) in r.iter().enumerate() {
                println!(
                    "[{}] {} | {} | {}",
                    i + 1,
                    sr.title,
                    sr.url,
                    sr.snippet
                        .chars()
                        .take(SNIPPET_PREVIEW_CHARS)
                        .collect::<String>()
                );
            }
            assert!(
                !r.is_empty(),
                "Google returned 0 results: parser or stealth broken"
            );
        }
        Err(e) => {
            panic!("Google search failed: {}", e);
        }
    }
}

async fn google_diagnostic() {
    let browser = crate::browser::HeadlessBrowser::global().await.unwrap();
    let navigation = browser.fetch_rendered("https://www.google.com/search?q=Rust%20programming%20language&hl=en&gl=us&pws=0&udm=14&filter=0").await;
    eprintln!(
        "Google navigation probe error: {:?}",
        navigation.as_ref().err()
    );
    if let (Ok(html), Ok(directory)) = (navigation, std::env::var("N2_SEARCH_FIXTURE_DIR")) {
        std::fs::write(
            std::path::Path::new(&directory).join("google-live.html"),
            html,
        )
        .expect("Write explicit search fixture");
    }
}
