// n2-kraken: auto-study orchestrator — search → crawl → extract → verify
use super::config::StudyConfig;
use super::crawler::crawl_pages;
use super::embedder::Embedder;
use super::error::KrakenError;
use super::extractor::extract_claims;
use super::fetcher::KrakenFetcher;
use super::search::search;
use super::types::{
    AutoStudyResult, Claim, CrawlReport, ResearchReport, VerificationStatus, VerifiedFact,
};
use super::verifier::verify_claims;

/// Full auto-study pipeline: search the web, crawl pages, extract claims, verify facts.
/// Returns summary stats + verified facts. Mimir storage is the consumer's responsibility.
pub async fn auto_study(
    topic: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
    embedder: Option<&dyn Embedder>,
) -> Result<(AutoStudyResult, Vec<VerifiedFact>), KrakenError> {
    let report = research(topic, config, fetcher, embedder).await?;
    Ok((report.summary, report.facts))
}

pub async fn research(
    topic: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
    embedder: Option<&dyn Embedder>,
) -> Result<ResearchReport, KrakenError> {
    config.validate()?;
    super::config::validate_query(topic)?;
    tokio::time::timeout(
        std::time::Duration::from_millis(config.total_timeout_ms),
        study_inner(topic, config, fetcher, embedder),
    )
    .await
    .map_err(|_| KrakenError::Timeout(config.total_timeout_ms))?
}

async fn study_inner(
    topic: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
    embedder: Option<&dyn Embedder>,
) -> Result<ResearchReport, KrakenError> {
    // 1. Search
    let results = search(topic, config, fetcher).await?;
    let search_count = results.len();

    if results.is_empty() {
        return Ok(empty_study(topic));
    }

    // 2. Crawl top pages
    let urls: Vec<String> = results.iter().map(|r| r.url.clone()).collect();
    let crawled = crawl_pages(&urls, config, fetcher).await?;
    analyze_pages(topic, search_count, crawled, config, embedder).await
}

async fn analyze_pages(
    topic: &str,
    search_count: usize,
    crawled: CrawlReport,
    config: &StudyConfig,
    embedder: Option<&dyn Embedder>,
) -> Result<ResearchReport, KrakenError> {
    let pages = crawled.pages;
    let pages_count = pages.len();

    // 3. Extract claims from each page
    let claims_by_source: Vec<Vec<Claim>> = pages
        .iter()
        .map(|page| extract_claims(&page.content, topic, &page.url))
        .collect();

    let total_claims: usize = claims_by_source.iter().map(|v| v.len()).sum();

    // 4. Verify across sources
    let (facts, semantic_mode) =
        verify_claims(&claims_by_source, config.min_confidence, embedder).await?;

    let result = summarize(
        topic,
        semantic_mode,
        search_count,
        pages_count,
        total_claims,
        &facts,
    );

    Ok(ResearchReport {
        summary: result,
        facts,
        pages,
        failures: crawled.failures,
    })
}

fn summarize(
    topic: &str,
    semantic_mode: bool,
    search_count: usize,
    pages_count: usize,
    total_claims: usize,
    facts: &[VerifiedFact],
) -> AutoStudyResult {
    let verified = facts
        .iter()
        .filter(|f| f.status == VerificationStatus::Verified)
        .count();
    let pending = facts
        .iter()
        .filter(|f| f.status == VerificationStatus::Pending)
        .count();
    let rejected = facts
        .iter()
        .filter(|f| {
            matches!(
                f.status,
                VerificationStatus::Unverified | VerificationStatus::Flagged
            )
        })
        .count();

    AutoStudyResult {
        topic: topic.to_string(),
        semantic_mode,
        search_results: search_count,
        pages_crawled: pages_count,
        claims_extracted: total_claims,
        facts_verified: verified,
        facts_pending: pending,
        facts_rejected: rejected,
    }
}

fn empty_study(topic: &str) -> ResearchReport {
    ResearchReport {
        summary: AutoStudyResult {
            topic: topic.to_string(),
            semantic_mode: false,
            search_results: 0,
            pages_crawled: 0,
            claims_extracted: 0,
            facts_verified: 0,
            facts_pending: 0,
            facts_rejected: 0,
        },
        facts: Vec::new(),
        pages: Vec::new(),
        failures: Vec::new(),
    }
}

const CATEGORY_KEYWORDS: &[(&[&str], &str)] = &[
    (
        &[
            "react", "vue", "angular", "css", "html", "dom", "frontend", "tailwind",
        ],
        "frontend",
    ),
    (
        &[
            "node", "express", "django", "flask", "spring", "backend", "api", "rest", "graphql",
        ],
        "backend",
    ),
    (
        &[
            "rust",
            "python",
            "java",
            "typescript",
            "golang",
            "kotlin",
            "swift",
            "c++",
        ],
        "programming_language",
    ),
    (
        &[
            "sql", "postgres", "mysql", "mongodb", "redis", "database", "sqlite",
        ],
        "database",
    ),
    (
        &[
            "docker",
            "kubernetes",
            "ci/cd",
            "aws",
            "azure",
            "gcp",
            "terraform",
            "devops",
        ],
        "devops",
    ),
    (
        &[
            "llm",
            "gpt",
            "transformer",
            "neural",
            "machine learning",
            "ai",
            "embedding",
        ],
        "ai_ml",
    ),
    (
        &[
            "security",
            "encryption",
            "auth",
            "oauth",
            "jwt",
            "vulnerability",
            "xss",
        ],
        "security",
    ),
    (
        &["figma", "ux", "ui", "design", "accessibility", "responsive"],
        "design",
    ),
    (
        &[
            "test", "jest", "pytest", "cypress", "selenium", "coverage", "tdd",
        ],
        "testing",
    ),
];

/// Detect study category from topic string.
pub fn detect_study_category(topic: &str) -> &'static str {
    let lower = topic.to_lowercase();

    for &(keywords, category) in CATEGORY_KEYWORDS {
        for kw in keywords {
            if crate::text::contains_keyword_bounded(&lower, kw) {
                return category;
            }
        }
    }

    "general"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retrieved_pages_flow_through_extraction_and_source_agreement() {
        use crate::types::CrawledPage;
        use sha2::{Digest, Sha256};
        let content = "Rust is a programming language with memory safety and reliable concurrency.";
        let pages = ["one", "two", "three"].map(|host| CrawledPage {
            url: format!("https://{host}.example/article"),
            title: "Source fixture".into(),
            content: content.into(),
            fetched_at: "1700000000".into(),
            content_length: content.len(),
            content_hash: Sha256::digest(content.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            truncated: false,
        });
        let report = analyze_pages(
            "Rust memory safety",
            3,
            CrawlReport {
                pages: pages.to_vec(),
                failures: Vec::new(),
            },
            &StudyConfig::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(report.summary.pages_crawled, 3);
        assert_eq!(report.summary.claims_extracted, 3);
        assert_eq!(report.summary.facts_verified, 1);
        assert!(!report.summary.semantic_mode);
        assert_eq!(report.facts[0].claim, content);
        assert_eq!(report.facts[0].sources.len(), 3);
        assert_eq!(report.pages.len(), 3);
    }

    #[test]
    fn test_detect_category() {
        assert_eq!(
            detect_study_category("Rust async patterns"),
            "programming_language"
        );
        assert_eq!(
            detect_study_category("React hooks best practices"),
            "frontend"
        );
        assert_eq!(
            detect_study_category("Docker container networking"),
            "devops"
        );
        assert_eq!(detect_study_category("cooking recipes"), "general");
    }

    #[test]
    fn test_detect_category_word_boundary() {
        // Regression: short keywords must not match unrelated substrings.
        // 'ai' must not catch "maintain" or "training", 'ml' must not catch
        // 'html', 'dom' must not catch "freedom" or "domain", 'sql' must not
        // catch "msql" fragments.
        assert_eq!(
            detect_study_category("maintaining legacy systems"),
            "general",
            "'ai' should not hit 'maintaining'"
        );
        assert_eq!(
            detect_study_category("training programme for staff"),
            "general",
            "'ai' should not hit 'training'"
        );
        assert_eq!(
            detect_study_category("freedom of expression"),
            "general",
            "'dom' should not hit 'freedom'"
        );

        // Positive: the real intended keywords still classify
        assert_eq!(detect_study_category("ai safety research"), "ai_ml");
        assert_eq!(detect_study_category("llm fine tuning"), "ai_ml");
        assert_eq!(
            detect_study_category("dom manipulation in javascript"),
            "frontend"
        );
    }
}
