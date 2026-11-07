// Explicit installed-Chrome probe; only temporary local fixtures and the owned browser are used.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "Requires installed Chrome; run explicitly during the browser audit"]
async fn actual_browser_rejects_nonpublic_navigation_and_closes_cancelled_pages() {
    let (address, hits, fixture) = private_fixture().await;
    let browser = HeadlessBrowser::launch().await.unwrap();
    let before = page_count(&browser).await;
    cookies_are_scoped_before_navigation(&browser).await;
    reads_embedded_public_content(&browser).await;
    let private = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        browser.fetch_rendered(&format!("http://{address}/page")),
    )
    .await;
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        browser.fetch_rendered("data:text/html,<main>fixture</main>"),
    )
    .await;
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(400),
        browser.fetch_rendered(&format!("http://{address}/hold")),
    )
    .await;
    // Cancel while creation may still be in progress, independently of URL rejection.
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        let _page = OwnedPage::open(browser.browser.clone()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    close_owned_context_and_popup(&browser).await;
    let after = page_count(&browser).await;
    close_browser(&browser).await;
    drop(browser);
    fixture.abort();
    actual_public_browser_renders_https_and_cleaned_content_through_the_proxy().await;
    assert!(
        matches!(private, Ok(Err(_)))
            && matches!(data, Ok(Err(_)))
            && hits.load(Ordering::SeqCst) == 0
            && after <= before,
        "private={private:?}, data={data:?}, fixture_hits={}, pages_before={before}, pages_after={after}",
        hits.load(Ordering::SeqCst)
    );
}

async fn actual_public_browser_renders_https_and_cleaned_content_through_the_proxy() {
    let browser = HeadlessBrowser::global().await.unwrap();
    let profile_path = browser
        .profile
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .path()
        .to_owned();
    let tunnels_before = super::super::public_proxy::ESTABLISHED.load(Ordering::SeqCst);
    let html = browser.fetch_rendered("https://example.com/").await;
    let cleaned = browser.fetch_cleaned("https://example.com/").await;
    let pages = page_count(browser).await;
    shutdown_with_busy_lock(browser).await;
    assert!(
        html.as_ref()
            .is_ok_and(|html| html.contains("Example Domain")),
        "{html:?}"
    );
    assert!(
        cleaned
            .as_ref()
            .is_ok_and(|page| page.title.contains("Example Domain")
                && page.content.contains("documentation")),
        "{cleaned:?}"
    );
    assert!(
        super::super::public_proxy::ESTABLISHED.load(Ordering::SeqCst) > tunnels_before,
        "Public render must traverse the guarded proxy"
    );
    assert_eq!(pages, 0, "Successful renders must close their pages");
    assert!(
        !profile_path.exists(),
        "Owned Chrome profile must be removed at shutdown"
    );
    assert!(
        browser
            .fetch_rendered("https://example.com/")
            .await
            .unwrap_err()
            .contains("stopping")
    );
}

async fn close_owned_context_and_popup(browser: &HeadlessBrowser) {
    use chromiumoxide::cdp::browser_protocol::target::CreateTargetParams;
    let page = OwnedPage::open(browser.browser.clone()).await.unwrap();
    page.set_content("<main>OWNED_PAGE</main><script>window.onbeforeunload=()=> 'stay'</script>")
        .await
        .unwrap();
    let targets = browser.browser.lock().await.fetch_targets().await.unwrap();
    let context = targets
        .iter()
        .find(|target| &target.target_id == page.target_id())
        .unwrap()
        .browser_context_id
        .clone()
        .unwrap();
    let popup = CreateTargetParams::builder()
        .url("about:blank")
        .browser_context_id(context.clone())
        .build()
        .unwrap();
    browser.browser.lock().await.new_page(popup).await.unwrap();
    drop(page);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let targets = browser.browser.lock().await.fetch_targets().await.unwrap();
    assert!(
        !targets
            .iter()
            .any(|target| target.browser_context_id.as_ref() == Some(&context)),
        "Cancellation must dispose the request context including its popup"
    );
}

async fn cookies_are_scoped_before_navigation(browser: &HeadlessBrowser) {
    use chromiumoxide::cdp::browser_protocol::network::GetCookiesParams;
    let page = OwnedPage::open(browser.browser.clone()).await.unwrap();
    set_google_cookie(&page).await.unwrap();
    let request = || {
        GetCookiesParams::builder()
            .url("https://www.google.com/")
            .build()
    };
    let stored = page.execute(request()).await.unwrap().result.cookies;
    let other = OwnedPage::open(browser.browser.clone()).await.unwrap();
    let separate = other.execute(request()).await.unwrap().result.cookies;
    other.close().await;
    page.close().await;
    assert!(
        stored
            .iter()
            .any(|cookie| cookie.name == "CONSENT" && cookie.value == "PENDING+999")
    );
    assert!(!separate.iter().any(|cookie| cookie.name == "CONSENT"));
}

async fn reads_embedded_public_content(browser: &HeadlessBrowser) {
    let page = OwnedPage::open(browser.browser.clone()).await.unwrap();
    navigate(&page, "https://example.com/").await.unwrap();
    page.set_content("<main>Owned host text</main><iframe src='https://example.com/'></iframe>")
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let content =
                super::super::browser_extract::extract(&page, "https://example.com/").await?;
            if content.iframe_count > 0 {
                return Ok::<_, String>(content);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await;
    page.close().await;
    let content = result
        .expect("Iframe extraction deadline")
        .expect("Iframe extraction");
    assert_eq!(content.iframe_count, 1);
    assert!(
        content.content.contains("Embedded iframe") && content.content.contains("documentation"),
        "{}",
        content.content.chars().take(800).collect::<String>()
    );
}

async fn page_count(browser: &HeadlessBrowser) -> usize {
    browser
        .browser
        .lock()
        .await
        .fetch_targets()
        .await
        .unwrap()
        .into_iter()
        .filter(|target| target.r#type == "page" && !target.url.starts_with("chrome://"))
        .count()
}

async fn close_browser(browser: &HeadlessBrowser) {
    {
        let mut owned = browser.browser.lock().await;
        let _ = owned.close().await;
        if tokio::time::timeout(std::time::Duration::from_secs(3), owned.wait())
            .await
            .is_err()
        {
            let _ = owned.kill().await;
        }
    }
}

async fn shutdown_with_busy_lock(browser: &HeadlessBrowser) {
    // Exercise the Windows process-owner fallback even when a CDP operation holds its mutex.
    #[cfg(windows)]
    {
        let mut busy = browser.browser.lock().await;
        super::shutdown().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            busy.get_mut_child().unwrap().try_wait().unwrap().is_some(),
            "Owned Chrome must exit despite a busy CDP lock"
        );
    }
    #[cfg(not(windows))]
    {
        super::shutdown().await;
        assert!(*browser.shutdown.borrow());
    }
}

async fn private_fixture() -> (
    std::net::SocketAddr,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let count = hits.clone();
    let fixture = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let mut bytes = [0; 4096];
                if let Ok(size) = stream.read(&mut bytes).await {
                    if size == 0 {
                        return;
                    }
                    count.fetch_add(1, Ordering::SeqCst);
                    if String::from_utf8_lossy(&bytes[..size]).contains("/hold") {
                        let _ = stream.read(&mut bytes).await;
                        return;
                    }
                    let html = "<title>Owned fixture</title><main>PRIVATE_FIXTURE_CONTENT</main>";
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        html.len(),
                        html
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                }
            });
        }
    });
    (address, hits, fixture)
}
