// n2-kraken: shared data types for the auto-study pipeline
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawledPage {
    pub url: String,
    pub title: String,
    pub content: String,
    pub fetched_at: String,
    pub content_length: usize,
    pub content_hash: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlFailure {
    pub url: String,
    pub stage: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrawlReport {
    pub pages: Vec<CrawledPage>,
    pub failures: Vec<CrawlFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub summary: AutoStudyResult,
    pub pages: Vec<CrawledPage>,
    pub failures: Vec<CrawlFailure>,
    pub facts: Vec<VerifiedFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    pub source: String,
    pub context: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VerificationStatus {
    Verified,
    Pending,
    Unverified,
    Flagged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedFact {
    pub claim: String,
    pub confidence: f64,
    pub sources: Vec<String>,
    pub status: VerificationStatus,
    pub contradiction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoStudyResult {
    pub topic: String,
    pub semantic_mode: bool,
    pub search_results: usize,
    pub pages_crawled: usize,
    pub claims_extracted: usize,
    pub facts_verified: usize,
    pub facts_pending: usize,
    pub facts_rejected: usize,
}
