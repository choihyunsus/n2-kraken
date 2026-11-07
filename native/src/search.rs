// Ordered search fallback returns the first usable result set and preserves genuine empty searches.
use super::config::StudyConfig;
use super::error::KrakenError;
use super::fetcher::KrakenFetcher;
use super::types::SearchResult;

pub async fn search(
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    config.validate()?;
    super::config::validate_query(query)?;
    tokio::time::timeout(
        std::time::Duration::from_millis(config.total_timeout_ms),
        search_inner(query, config, fetcher),
    )
    .await
    .map_err(|_| KrakenError::Timeout(config.total_timeout_ms))?
}

async fn search_inner(
    query: &str,
    config: &StudyConfig,
    fetcher: &KrakenFetcher,
) -> Result<Vec<SearchResult>, KrakenError> {
    let mut errors = Vec::new();
    let mut valid_empty = false;
    for engine in &config.engines {
        let attempt = super::search_engines::query(engine, query, config, fetcher);
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(config.timeout_ms), attempt)
                .await
                .unwrap_or_else(|_| Err(KrakenError::Timeout(config.timeout_ms)));
        match result {
            Ok(results) if !results.is_empty() => {
                tracing::info!(
                    engine,
                    count = results.len(),
                    "Search returned usable results"
                );
                return Ok(results);
            }
            Ok(_) => {
                valid_empty = true;
            }
            Err(error) => {
                tracing::warn!(
                    engine,
                    "Search engine failed; considering the next configured engine"
                );
                errors.push(format!("{engine}: {error}"));
            }
        }
    }
    if valid_empty {
        Ok(Vec::new())
    } else {
        Err(KrakenError::Parse(format!(
            "No search engine succeeded: {}",
            errors.join("; ")
        )))
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
