// n2-kraken: study pipeline configuration
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StudyConfig {
    pub max_results: usize,
    pub max_crawl_pages: usize,
    pub min_confidence: f64,
    pub max_content_length: usize,
    pub timeout_ms: u64,
    pub total_timeout_ms: u64,
    pub time_range: String,
    /// Search engines to use in cascade order.
    /// Supported: "ddg", "bing", "naver", "google", "searxng"
    /// Env: KRAKEN_ENGINES (comma-separated, default: "ddg,bing,naver")
    /// SearXNG and Google are opt-in. SearXNG uses the explicitly configured service.
    pub engines: Vec<String>,
    /// SearXNG instance URL (env: SEARXNG_URL, no default; explicitly configured by the operator)
    pub searxng_url: String,
}

impl Default for StudyConfig {
    fn default() -> Self {
        Self {
            max_results: 10,
            max_crawl_pages: Self::default_max_crawl_pages(),
            min_confidence: 0.6,
            max_content_length: 5_000,
            timeout_ms: 30_000,
            total_timeout_ms: 180_000,
            time_range: "3m".to_string(),
            engines: Self::default_engines(),
            searxng_url: Self::default_searxng_url(),
        }
    }
}

impl StudyConfig {
    pub fn validate(&self) -> Result<(), super::error::KrakenError> {
        let valid = (1..=40).contains(&self.max_results)
            && (1..=20).contains(&self.max_crawl_pages)
            && self.min_confidence.is_finite()
            && (0.5..=1.0).contains(&self.min_confidence)
            && (50..=100_000).contains(&self.max_content_length)
            && (100..=120_000).contains(&self.timeout_ms)
            && (100..=600_000).contains(&self.total_timeout_ms)
            && ["", "1w", "1m", "3m", "6m", "1y"].contains(&self.time_range.as_str())
            && (1..=5).contains(&self.engines.len())
            && self.engines.iter().all(|engine| {
                ["ddg", "bing", "naver", "google", "searxng"].contains(&engine.as_str())
            })
            && self
                .engines
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.engines.len()
            && self.searxng_url.len() <= 8192;
        if !valid {
            return Err(super::error::KrakenError::InvalidInput(
                "Research configuration exceeds supported limits".into(),
            ));
        }
        Ok(())
    }

    /// Pages crawled per study run. Default 5 keeps token cost bounded; raise via
    /// KRAKEN_MAX_CRAWL_PAGES (clamped 1..=20) when source pool stability matters
    /// more than per-call cost.
    pub fn default_max_crawl_pages() -> usize {
        std::env::var("KRAKEN_MAX_CRAWL_PAGES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(5)
            .clamp(1, 20)
    }

    /// Load engine list from KRAKEN_ENGINES env var, fallback to "ddg,bing,naver"
    pub fn default_engines() -> Vec<String> {
        std::env::var("KRAKEN_ENGINES")
            .unwrap_or_else(|_| "ddg,bing,naver".to_string())
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// SearXNG instance URL from SEARXNG_URL env var
    pub fn default_searxng_url() -> String {
        std::env::var("SEARXNG_URL").unwrap_or_default()
    }
}

pub(crate) fn validate_query(query: &str) -> Result<(), super::error::KrakenError> {
    if query.trim().is_empty() || query.len() > 4096 || query.chars().any(char::is_control) {
        return Err(super::error::KrakenError::InvalidInput(
            "Expected a nonempty query within 4096 UTF-8 bytes".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_limit_can_be_reduced_without_changing_the_crawl_limit() {
        let config: StudyConfig = serde_json::from_str(r#"{"max_results":1}"#).unwrap();
        assert_eq!(config.max_results, 1);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn public_configuration_rejects_invalid_limits_and_unknown_fields() {
        for json in [
            r#"{"max_results":0}"#,
            r#"{"max_crawl_pages":21}"#,
            r#"{"min_confidence":0.4}"#,
            r#"{"max_content_length":100001}"#,
            r#"{"timeout_ms":0}"#,
            r#"{"total_timeout_ms":600001}"#,
            r#"{"engines":[]}"#,
            r#"{"engines":["ddg","ddg"]}"#,
            r#"{"engines":["unknown"]}"#,
            r#"{"time_range":"invalid"}"#,
        ] {
            assert!(
                serde_json::from_str::<StudyConfig>(json)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{json}"
            );
        }
        assert!(serde_json::from_str::<StudyConfig>(r#"{"typo":1}"#).is_err());
        for query in ["", "   ", "line\nbreak", "null\0byte"] {
            assert!(validate_query(query).is_err());
        }
        assert!(validate_query(&"a".repeat(4097)).is_err());
        assert!(validate_query("한글 Rust ownership").is_ok());
    }
}
