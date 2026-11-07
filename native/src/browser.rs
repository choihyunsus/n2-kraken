// N2-Kraken: owned headless Chrome for public web rendering.
// Browser compatibility patches do not guarantee bypassing website challenges.

use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};
use tracing::{info, warn};

use super::browser_page::{OwnedPage, google_url, navigate};
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::emulation::SetUserAgentOverrideParams;
use chromiumoxide::cdp::browser_protocol::network::{CookieParam, SetCookiesParams};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Global headless browser instance (lazy singleton).
static HEADLESS: OnceCell<Arc<HeadlessBrowser>> = OnceCell::const_new();

#[cfg(test)]
#[path = "browser_security_tests.rs"]
mod security_tests;

/// Stealth JS payload — injected before every page load via CDP.
/// Compatibility patches for common website checks, without a bypass guarantee.
const STEALTH_JS: &str = include_str!("scripts/stealth.js");

const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

/// Output of `fetch_cleaned` -- already-extracted markdown, no HTML parsing
/// needed downstream. Callers can wrap this directly into `CrawledPage`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CleanedPage {
    pub title: String,
    pub url: String,
    pub content: String,
    pub iframe_count: usize,
    pub truncated: bool,
}

pub struct HeadlessBrowser {
    browser: Arc<Mutex<Browser>>,
    capacity: tokio::sync::Semaphore,
    shutdown: tokio::sync::watch::Sender<bool>,
    #[cfg(windows)]
    process_tree: std::sync::Mutex<Option<crate::process_tree::ProcessTree>>,
    profile: std::sync::Mutex<Option<super::browser_profile::BrowserProfile>>,
}

impl HeadlessBrowser {
    /// Get or launch the global headless browser instance.
    pub async fn global() -> Result<&'static Arc<HeadlessBrowser>, String> {
        HEADLESS
            .get_or_try_init(|| async { Self::launch().await.map(Arc::new) })
            .await
            .map_err(|e| format!("Headless browser init failed: {}", e))
    }

    async fn bounded<T>(
        &self,
        url: &str,
        work: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let _permit = self
            .capacity
            .try_acquire()
            .map_err(|_| "Browser render capacity reached".to_owned())?;
        let mut stopping = self.shutdown.subscribe();
        if *stopping.borrow() {
            return Err("Browser is stopping".into());
        }
        tokio::select! {
            _ = stopping.changed() => Err("Browser is stopping".into()),
            result = tokio::time::timeout(RENDER_TIMEOUT, async {
                super::public_proxy::validate(url).await.map_err(|e|e.to_string())?;
                work.await
            }) => result.map_err(|_|"Browser render timed out".to_owned())?,
        }
    }
    pub async fn fetch_cleaned(&self, url: &str) -> Result<CleanedPage, String> {
        self.bounded(url, self.fetch_cleaned_inner(url)).await
    }
    pub async fn fetch_rendered(&self, url: &str) -> Result<String, String> {
        self.fetch_document(url).await.map(|document| document.html)
    }

    pub async fn fetch_document(
        &self,
        url: &str,
    ) -> Result<super::fetcher::FetchedDocument, String> {
        self.bounded(url, self.fetch_rendered_inner(url)).await
    }

    async fn launch() -> Result<Self, String> {
        let proxy = super::public_proxy::address()
            .await
            .map_err(|e| e.to_string())?;
        let profile = super::browser_profile::BrowserProfile::create()
            .map_err(|e| format!("Chrome profile creation failed: {e}"))?;
        let config = super::browser_config::build(proxy, profile.path())?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("Chrome launch failed: {}", e))?;
        #[cfg(windows)]
        let (browser, process_tree) = contain_browser(browser).await?;

        // Spawn event handler in background
        tokio::spawn(async move {
            loop {
                match handler.next().await {
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => {
                        warn!("[KRAKEN:HEADLESS] Handler error: {}", e);
                        break;
                    }
                    None => {
                        info!("[KRAKEN:HEADLESS] Handler stream ended");
                        break;
                    }
                }
            }
        });

        info!("[KRAKEN:HEADLESS] Chrome launched (stealth mode)");
        Ok(Self {
            browser: Arc::new(Mutex::new(browser)),
            capacity: tokio::sync::Semaphore::new(3),
            shutdown: tokio::sync::watch::channel(false).0,
            #[cfg(windows)]
            process_tree: std::sync::Mutex::new(Some(process_tree)),
            profile: std::sync::Mutex::new(Some(profile)),
        })
    }

    /// Fetch a page and extract cleaned markdown text directly from the live
    /// DOM via CDP TreeWalker. No HTML parsing needed downstream -- the
    /// returned `CleanedPage.content` is already noise-filtered and
    /// role-aware. Selected child iframes are read in isolated worlds.
    /// Truncation is reported when bounds or extraction failures omit content.
    async fn fetch_cleaned_inner(&self, url: &str) -> Result<CleanedPage, String> {
        let page = self.prepare_page(url).await?;
        navigate(&page, url).await?;
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        let result = super::browser_extract::extract(&page, url).await;
        page.close().await;
        result
    }

    async fn prepare_page(&self, url: &str) -> Result<OwnedPage, String> {
        let page = OwnedPage::open(self.browser.clone()).await?;

        page.execute(SetUserAgentOverrideParams::new(CHROME_UA))
            .await
            .map_err(|e| format!("UA override failed: {}", e))?;

        // Stealth: inject anti-detection JS before any page load
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_JS))
            .await
            .map_err(|e| format!("Stealth JS injection failed: {}", e))?;

        // Google: 2-step approach — warm up session first, then search
        if google_url(url) {
            // Step 1: Visit google.com homepage to establish cookies + pass JS challenge
            set_google_cookie(&page).await?;

            navigate(&page, "https://www.google.com/").await?;
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

            info!("[KRAKEN:HEADLESS] Google warmup complete, navigating to search");
        }

        Ok(page)
    }

    /// Fetch a page with full JS rendering + stealth patches.
    /// Injects anti-detection JS before navigation, spoofs User-Agent via CDP.
    async fn fetch_rendered_inner(
        &self,
        url: &str,
    ) -> Result<super::fetcher::FetchedDocument, String> {
        let page = self.prepare_page(url).await?;

        // Navigate to target URL
        navigate(&page, url).await?;

        // Wait for JS-rendered content
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        let html: String = page.evaluate("(()=>{const html=document.documentElement.outerHTML;if(html.length>2000000)throw Error('Rendered HTML exceeds limit');return html})()")
            .await.map_err(|e|format!("Content extraction failed: {e}"))?.into_value().map_err(|e|format!("Content decoding failed: {e}"))?;

        let current = page
            .url()
            .await
            .map_err(|_| "Rendered source URL unavailable")?
            .ok_or("Rendered source URL unavailable")?;
        let current = crate::destination::parse(&current)?;
        // Close the page to free resources
        let _ = page.close().await;

        Ok(super::fetcher::FetchedDocument {
            url: current.to_string(),
            html,
        })
    }
}

async fn set_google_cookie(page: &OwnedPage) -> Result<(), String> {
    let consent = CookieParam::builder()
        .url("https://www.google.com/")
        .domain(".google.com")
        .name("CONSENT")
        .value("PENDING+999")
        .path("/")
        .build()
        .map_err(|e| format!("Cookie build failed: {}", e))?;
    page.execute(SetCookiesParams::new(vec![consent]))
        .await
        .map_err(|e| format!("Cookie injection failed: {}", e))?;
    Ok(())
}

#[cfg(windows)]
async fn contain_browser(
    mut browser: Browser,
) -> Result<(Browser, crate::process_tree::ProcessTree), String> {
    let attached = browser
        .get_mut_child()
        .ok_or_else(|| std::io::Error::other("Missing owned Chrome process"))
        .and_then(|child| crate::process_tree::ProcessTree::attach(&child.inner));
    match attached {
        Ok(tree) => Ok((browser, tree)),
        Err(error) => {
            let _ = browser.kill().await;
            Err(format!("Chrome process containment failed: {error}"))
        }
    }
}

/// Stop only the Chrome process launched by this engine and its network proxy.
pub async fn shutdown() {
    if let Some(owner) = HEADLESS.get() {
        owner.shutdown.send_replace(true);
        if let Ok(mut browser) =
            tokio::time::timeout(std::time::Duration::from_secs(2), owner.browser.lock()).await
        {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), browser.close()).await;
            if tokio::time::timeout(std::time::Duration::from_secs(1), browser.wait())
                .await
                .is_err()
            {
                let _ = browser.kill().await;
            }
        }
        #[cfg(windows)]
        if let Ok(mut tree) = owner.process_tree.lock() {
            tree.take();
        }
        let profile = owner
            .profile
            .lock()
            .ok()
            .and_then(|mut profile| profile.take());
        let _ = tokio::task::spawn_blocking(move || drop(profile)).await;
    }
    super::public_proxy::shutdown();
}

#[cfg(test)]
mod iframe_tests {
    use super::*;

    /// Live measurement: hit iframe-heavy URLs and report walk_iframes() yield.
    /// Topic set is intentionally varied (docs demos, embedded media, news) so
    /// we can see how MAX_IFRAME_FETCH=3 holds up against real pages.
    #[tokio::test]
    #[ignore]
    async fn measure_iframe_heavy_live() {
        const URLS: &[&str] = &[
            "https://www.w3schools.com/html/html_iframe.asp",
            "https://www.w3schools.com/tags/tag_iframe.asp",
            "https://en.wikipedia.org/wiki/HTML_element",
        ];

        let browser = match HeadlessBrowser::global().await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[IFRAME-BENCH] browser unavailable: {} -- skipping", e);
                return;
            }
        };

        println!(
            "\n=== IFRAME-HEAVY MEASUREMENT (MAX_IFRAME_FETCH={}) ===",
            super::super::browser_extract::MAX_IFRAMES
        );
        let mut total_iframes = 0usize;
        let mut total_chars = 0usize;
        for url in URLS {
            match browser.fetch_cleaned(url).await {
                Ok(cleaned) => {
                    println!(
                        "  {} -- iframe_count={} content_chars={}",
                        url,
                        cleaned.iframe_count,
                        cleaned.content.len()
                    );
                    total_iframes += cleaned.iframe_count;
                    total_chars += cleaned.content.len();
                }
                Err(e) => {
                    println!("  {} -- ERROR: {}", url, e);
                }
            }
        }
        super::shutdown().await;
        println!(
            "  TOTAL iframes={} chars={} (avg iframes/page={:.1})",
            total_iframes,
            total_chars,
            total_iframes as f64 / URLS.len() as f64
        );
    }
}
