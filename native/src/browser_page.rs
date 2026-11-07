// Isolated request contexts close their pages, popups and storage even on cancellation.
use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParams,
};
use chromiumoxide::{browser::Browser, page::Page};
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, Semaphore};

type SharedBrowser = Arc<Mutex<Browser>>;
const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
static CREATIONS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

struct OwnedContext {
    browser: SharedBrowser,
    id: Option<BrowserContextId>,
}
impl OwnedContext {
    async fn close(&mut self) {
        if let Some(id) = self.id.as_ref() {
            let result = tokio::time::timeout(CLEANUP_TIMEOUT, async {
                self.browser
                    .lock()
                    .await
                    .dispose_browser_context(id.clone())
                    .await
            })
            .await;
            if matches!(result, Ok(Ok(()))) {
                self.id.take();
            }
        }
    }
}
impl Drop for OwnedContext {
    fn drop(&mut self) {
        if let (Some(id), Ok(runtime)) = (self.id.take(), tokio::runtime::Handle::try_current()) {
            let browser = self.browser.clone();
            runtime.spawn(async move {
                let _ = tokio::time::timeout(CLEANUP_TIMEOUT, async {
                    browser.lock().await.dispose_browser_context(id).await
                })
                .await;
            });
        }
    }
}

pub(super) struct OwnedPage {
    page: Page,
    context: OwnedContext,
}
impl OwnedPage {
    pub(super) async fn open(browser: SharedBrowser) -> Result<Self, String> {
        let permit = CREATIONS
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Browser page creation capacity reached")?;
        // A dropped caller does not cancel CDP creation before we know which context to dispose.
        // The detached task retains its capacity permit; its undelivered result drops the context.
        tokio::spawn(async move {
            let _permit = permit;
            let locked = browser.lock().await;
            let id = locked
                .create_browser_context(
                    CreateBrowserContextParams::builder()
                        .dispose_on_detach(true)
                        .build(),
                )
                .await
                .map_err(|e| format!("Browser context creation failed: {e}"))?;
            let context = OwnedContext {
                browser: browser.clone(),
                id: Some(id.clone()),
            };
            let params = CreateTargetParams::builder()
                .url("about:blank")
                .browser_context_id(id)
                .build()
                .map_err(|e| format!("Page options failed: {e}"))?;
            let page = locked
                .new_page(params)
                .await
                .map_err(|e| format!("Page creation failed: {e}"))?;
            Ok(Self { page, context })
        })
        .await
        .map_err(|_| "Browser page task stopped".to_owned())?
    }
    pub(super) async fn close(mut self) {
        self.context.close().await;
    }
}
impl std::ops::Deref for OwnedPage {
    type Target = Page;
    fn deref(&self) -> &Page {
        &self.page
    }
}

pub(super) async fn navigate(page: &Page, url: &str) -> Result<(), String> {
    page.goto(url)
        .await
        .map_err(|error| format!("Navigation failed: {error}"))?;
    let request = page
        .wait_for_navigation_response()
        .await
        .map_err(|error| format!("Navigation failed: {error}"))?;
    let response = request
        .as_ref()
        .and_then(|request| request.response.as_ref())
        .ok_or("Missing page response")?;
    if !(200..300).contains(&(response.status as u16)) {
        return Err(format!("Page request failed (HTTP {})", response.status));
    }
    Ok(())
}
pub(super) fn google_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| host == "google.com" || host.ends_with(".google.com"))
}
