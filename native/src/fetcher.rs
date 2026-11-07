// N2-Daebung: Kraken HTTP fetcher with Chrome emulation + SSRF guard.
use futures::StreamExt;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};
use wreq::header;
use wreq_util::Emulation;

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("Network request failed")]
    Network(#[from] wreq::Error),
    #[error("Timeout error")]
    Timeout,
    #[error("Unexpected HTTP status: {0}")]
    HttpStatus(u16),
    #[error("Size limit exceeded: {0}")]
    TooLarge(String),
    #[error("SSRF blocked: {0}")]
    SsrfBlocked(String),
}

/// Max concurrent HTTP requests
const MAX_CONCURRENT: usize = 2;
/// Min interval between requests (ms)
const MIN_INTERVAL_MS: u64 = 3000;

/// Chrome emulation header constants (kept in one place so the
/// pinned-client rebuild and the global client cannot drift).
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const CHROME_UA_BRANDS: &str =
    "\"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"";
const CHROME_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";
const CHROME_ACCEPT_LANG: &str = "en-US,en;q=0.9,ko-KR;q=0.8,ko;q=0.7";
const CHROME_CONSENT_COOKIE: &str = "CONSENT=YES+cb.20230501-14-p0.en+FX+387";

fn chrome_headers() -> header::HeaderMap {
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(CHROME_UA),
    );
    headers.insert(
        "sec-ch-ua",
        header::HeaderValue::from_static(CHROME_UA_BRANDS),
    );
    headers.insert("sec-ch-ua-mobile", header::HeaderValue::from_static("?0"));
    headers.insert(
        "sec-ch-ua-platform",
        header::HeaderValue::from_static("\"Windows\""),
    );
    headers.insert(
        "upgrade-insecure-requests",
        header::HeaderValue::from_static("1"),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static(CHROME_ACCEPT),
    );
    headers.insert(
        header::ACCEPT_LANGUAGE,
        header::HeaderValue::from_static(CHROME_ACCEPT_LANG),
    );
    headers.insert("sec-fetch-site", header::HeaderValue::from_static("none"));
    headers.insert(
        "sec-fetch-mode",
        header::HeaderValue::from_static("navigate"),
    );
    headers.insert("sec-fetch-user", header::HeaderValue::from_static("?1"));
    headers.insert(
        "sec-fetch-dest",
        header::HeaderValue::from_static("document"),
    );
    headers
}

/// Global rate limiter and domain-scoped cookies. Connections are pinned per hop.
static GLOBAL_FETCHER: LazyLock<KrakenFetcher> = LazyLock::new(KrakenFetcher::new);

pub struct KrakenFetcher {
    cookies: Arc<wreq::cookie::Jar>,
    semaphore: Arc<Semaphore>,
    last_request: Arc<Mutex<Instant>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FetchedDocument {
    pub url: String,
    pub html: String,
}

impl Default for KrakenFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl KrakenFetcher {
    /// Get global singleton instance (production use)
    pub fn global() -> &'static Self {
        &GLOBAL_FETCHER
    }

    pub fn new() -> Self {
        let cookies = Arc::new(wreq::cookie::Jar::default());
        cookies.add(
            format!("{CHROME_CONSENT_COOKIE}; Domain=google.com; Path=/; Secure"),
            "https://www.google.com/",
        );
        Self {
            cookies,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            last_request: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(60))),
        }
    }

    /// Bound decompressed and decoded document bytes independently.
    const MAX_RESPONSE_SIZE: usize = 8 * 1024 * 1024;

    /// SEC: Validate URL is not targeting internal/private networks.
    pub async fn check_ssrf(url: &str) -> Result<String, FetchError> {
        crate::destination::resolve(url)
            .await
            .map(|(url, _)| url.to_string())
            .map_err(FetchError::SsrfBlocked)
    }

    /// Maximum redirect hops to follow (prevents infinite redirect loops)
    const MAX_REDIRECTS: usize = 5;

    /// Enforce minimum interval between HTTP requests (rate limiter)
    async fn enforce_interval(&self) {
        let mut last = self.last_request.lock().await;
        let elapsed = last.elapsed();
        let min = Duration::from_millis(MIN_INTERVAL_MS);
        if elapsed < min {
            tokio::time::sleep(min - elapsed).await;
        }
        *last = Instant::now();
    }

    pub async fn fetch_html(&self, url: &str) -> Result<String, FetchError> {
        self.fetch_document(url).await.map(|document| document.html)
    }

    pub async fn fetch_document(&self, url: &str) -> Result<FetchedDocument, FetchError> {
        tokio::time::timeout(Duration::from_secs(60), self.fetch_inner(url))
            .await
            .map_err(|_| FetchError::Timeout)?
    }

    async fn fetch_inner(&self, url: &str) -> Result<FetchedDocument, FetchError> {
        // Rate limiter: acquire semaphore permit + enforce interval
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| FetchError::Timeout)?;
        self.enforce_interval().await;

        let (mut current_url, mut client) = self.validated_client(url).await?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let jitter_ms = 800 + (nanos % 1700) as u64;
        tokio::time::sleep(Duration::from_millis(jitter_ms)).await;

        // The initial request plus at most MAX_REDIRECTS validated hops.
        for hop in 0..=Self::MAX_REDIRECTS {
            let resp = client.get(&current_url).send().await?;
            if !matches!(resp.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok(FetchedDocument {
                    url: current_url,
                    html: read_response(resp, Self::MAX_RESPONSE_SIZE).await?,
                });
            }
            if hop == Self::MAX_REDIRECTS {
                return Err(FetchError::SsrfBlocked("Too many redirects".into()));
            }
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    FetchError::SsrfBlocked("Redirect without Location header".into())
                })?;
            let base = url::Url::parse(&current_url)
                .map_err(|_| FetchError::SsrfBlocked("Bad redirect base".into()))?;
            let target = base
                .join(location)
                .map_err(|_| FetchError::SsrfBlocked("Bad redirect URL".into()))?;
            (current_url, client) = self.validated_client(target.as_str()).await?;
        }
        Err(FetchError::SsrfBlocked("Too many redirects".into()))
    }

    async fn validated_client(&self, input: &str) -> Result<(String, wreq::Client), FetchError> {
        let (parsed, addrs) = crate::destination::resolve(input)
            .await
            .map_err(FetchError::SsrfBlocked)?;
        let host = parsed
            .host_str()
            .ok_or_else(|| FetchError::SsrfBlocked("Missing host".into()))?;
        Ok((parsed.to_string(), self.build_pinned_client(host, &addrs)?))
    }

    /// No automatic redirects or proxy-side DNS may bypass the validated address.
    fn build_pinned_client(
        &self,
        host: &str,
        addrs: &[std::net::SocketAddr],
    ) -> Result<wreq::Client, FetchError> {
        let mut builder = wreq::Client::builder()
            .emulation(Emulation::Chrome131)
            .default_headers(chrome_headers())
            .cookie_provider(self.cookies.clone())
            .no_proxy()
            .timeout(Duration::from_secs(12))
            .redirect(wreq::redirect::Policy::none());
        if !addrs.is_empty() && !host.is_empty() {
            builder = builder.resolve_to_addrs(host.to_owned(), addrs.iter().copied());
        }
        builder
            .build()
            .map_err(|e| FetchError::SsrfBlocked(format!("pinned client build failed: {}", e)))
    }
}

/// Bound decompressed bytes while streaming, then decode the declared text charset.
async fn read_response(resp: wreq::Response, limit: usize) -> Result<String, FetchError> {
    if !resp.status().is_success() {
        return Err(FetchError::HttpStatus(resp.status().as_u16()));
    }
    if resp
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(FetchError::TooLarge(format!(
            "Response exceeds {limit} bytes"
        )));
    }
    let encoding = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .and_then(|value| {
            value
                .get_param(mime::CHARSET)
                .and_then(|charset| encoding_rs::Encoding::for_label(charset.as_str().as_bytes()))
        })
        .unwrap_or(encoding_rs::UTF_8);
    let mut stream = resp.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(FetchError::TooLarge(format!(
                "Response exceeds {limit} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    let (text, _, _) = encoding.decode(&body);
    if text.len() > limit {
        return Err(FetchError::TooLarge(format!(
            "Decoded text exceeds {limit} bytes"
        )));
    }
    Ok(text.into_owned())
}

#[cfg(test)]
#[path = "fetcher_tests.rs"]
mod tests;
