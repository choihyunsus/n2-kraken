// Chrome launch settings; chromiumoxide adds the double-dash prefix itself.
use chromiumoxide::browser::BrowserConfig;
#[cfg(target_os = "windows")]
use tracing::info;
/// Find Chrome executable on Windows.
#[cfg(target_os = "windows")]
fn find_chrome() -> Option<std::path::PathBuf> {
    let candidates = [
        std::env::var("PROGRAMFILES")
            .ok()
            .map(|p| format!("{p}\\Google\\Chrome\\Application\\chrome.exe")),
        std::env::var("PROGRAMFILES(X86)")
            .ok()
            .map(|p| format!("{p}\\Google\\Chrome\\Application\\chrome.exe")),
        Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string()),
        Some(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string()),
    ];
    for c in candidates.into_iter().flatten() {
        let p = std::path::PathBuf::from(&c);
        if p.exists() {
            info!("[KRAKEN:HEADLESS] Chrome found: {}", c);
            return Some(p);
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn find_chrome() -> Option<std::path::PathBuf> {
    // On Linux/macOS, chromiumoxide's default detection works fine
    None
}

pub(super) fn build(
    proxy: std::net::SocketAddr,
    profile: &std::path::Path,
) -> Result<BrowserConfig, String> {
    let mut builder = BrowserConfig::builder()
        .new_headless_mode()
        .disable_default_args()
        .arg(("proxy-server", format!("http://{proxy}").as_str()))
        .arg(("proxy-bypass-list", "<-loopback>"))
        .arg("disable-quic")
        .arg(("force-webrtc-ip-handling-policy", "disable_non_proxied_udp"))
        // Stealth-critical: no enable-automation flag (chromiumoxide default includes it)
        .arg(("disable-blink-features", "AutomationControlled"))
        // Stability & performance
        .arg("disable-dev-shm-usage")
        .arg("disable-background-networking")
        .arg("disable-sync")
        .arg("no-first-run")
        .arg("no-startup-window")
        .arg("disable-extensions")
        .arg("disable-infobars")
        .arg("disable-breakpad")
        .arg("disable-component-extensions-with-background-pages")
        .arg("disable-default-apps")
        .arg("disable-prompt-on-repost")
        .arg("disable-renderer-backgrounding")
        .arg("metrics-recording-only")
        .arg(("force-color-profile", "srgb"))
        .arg(("remote-debugging-port", "0"))
        .arg(("window-size", "1920,1080"))
        .arg("hide-scrollbars")
        .arg("mute-audio");

    // Honor the operator's explicit path on every supported operating system.
    let chrome = std::env::var_os("CHROME_PATH")
        .map(std::path::PathBuf::from)
        .or_else(find_chrome);
    if let Some(chrome) = chrome {
        builder = builder.chrome_executable(chrome);
    }

    builder = builder.user_data_dir(profile);

    builder
        .build()
        .map_err(|e| format!("BrowserConfig error: {e}"))
}
