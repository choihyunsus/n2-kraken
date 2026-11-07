// Standalone web retrieval and source-backed research engine.
pub mod browser;
mod browser_page;
pub mod config;
pub mod crawler;
mod destination;
pub mod embedder;
pub mod error;
pub mod extractor;
pub mod fetcher;
pub mod html;
pub mod math;
#[cfg(windows)]
mod process_tree;
pub(crate) mod public_proxy;
pub mod search;
mod search_engines;
pub mod serp;
pub mod study;
mod text;
pub mod types;
pub mod verifier;

pub use config::StudyConfig;
pub use error::KrakenError;
pub use fetcher::KrakenFetcher;
pub use html::{ExtractedDocument, HtmlToMarkdownConfig, parse_html_to_markdown};
pub use study::{auto_study, research};
pub use types::{
    AutoStudyResult, Claim, CrawlFailure, CrawlReport, CrawledPage, ResearchReport, SearchResult,
    VerificationStatus, VerifiedFact,
};

mod searxng;

mod browser_config;
mod browser_extract;
mod browser_profile;
