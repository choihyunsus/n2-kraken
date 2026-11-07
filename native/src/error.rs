// n2-kraken: unified error type
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KrakenError {
    #[error("Network request failed")]
    Network(#[from] wreq::Error),
    #[error("Fetch error: {0}")]
    Fetch(#[from] super::fetcher::FetchError),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Timeout after {0}ms")]
    Timeout(u64),
    #[error("No results found for '{0}'")]
    NoResults(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
