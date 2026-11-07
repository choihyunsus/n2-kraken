// Extract bounded text from the main page and selected non-advertising child frames.
use super::browser::CleanedPage;
use chromiumoxide::cdp::browser_protocol::page::{
    CreateIsolatedWorldParams, Frame, GetFrameTreeParams,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::page::Page;
use std::sync::LazyLock;

const EXTRACT_JS: &str = include_str!("scripts/extract.js");
pub(super) const MAX_IFRAMES: usize = 3;
const MAX_CONTENT_CHARS: usize = 2_000_000;
static SKIP: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)ads?[.-]|doubleclick|googlesyndication|adservice|pstatic.*glad|taboola|outbrain",
    )
    .expect("Static iframe filter")
});

pub(super) async fn extract(page: &Page, url: &str) -> Result<CleanedPage, String> {
    let raw: String = page
        .evaluate(EXTRACT_JS)
        .await
        .map_err(|e| format!("Main extract failed: {e}"))?
        .into_value()
        .map_err(|e| format!("Main result decode failed: {e}"))?;
    let main: serde_json::Value =
        serde_json::from_str(raw.as_str()).map_err(|e| format!("Main JSON parse failed: {e}"))?;
    let mut content = main["content"].as_str().unwrap_or_default().to_owned();
    let (frames, frames_truncated) = walk_frames(page).await.unwrap_or_else(|error| {
        tracing::warn!("[KRAKEN:HEADLESS] iframe walk failed: {error}");
        (Vec::new(), true)
    });
    for (url, text) in &frames {
        content.push_str(&format!("\n\n---\n[Embedded iframe: {url}]\n{text}"));
    }
    let mut title = main["title"].as_str().unwrap_or_default().to_owned();
    let truncated = frames_truncated
        || main["truncated"].as_bool().unwrap_or(true)
        || content.chars().count() > MAX_CONTENT_CHARS
        || title.len() > 8192;
    title.truncate(title.floor_char_boundary(8192.min(title.len())));
    Ok(CleanedPage {
        title,
        url: crate::destination::parse(main["url"].as_str().unwrap_or(url))?.to_string(),
        content: content.chars().take(MAX_CONTENT_CHARS).collect(),
        iframe_count: frames.len(),
        truncated,
    })
}

async fn walk_frames(page: &Page) -> Result<(Vec<(String, String)>, bool), String> {
    let tree = page
        .execute(GetFrameTreeParams::default())
        .await
        .map_err(|e| e.to_string())?;
    let children = tree.result.frame_tree.child_frames.unwrap_or_default();
    let mut truncated = children.len() > MAX_IFRAMES;
    let mut results = Vec::new();
    for child in children.into_iter().take(MAX_IFRAMES) {
        truncated |= child
            .child_frames
            .is_some_and(|children| !children.is_empty());
        let frame = child.frame;
        if crate::destination::parse(&frame.url).is_err() || SKIP.is_match(&frame.url) {
            truncated = true;
            continue;
        }
        match read_frame(page, &frame).await {
            Ok(Some((text, omitted))) => {
                truncated |= omitted;
                results.push((frame.url, text));
            }
            Ok(None) => truncated = true,
            Err(error) => {
                truncated = true;
                tracing::warn!("[KRAKEN:HEADLESS] iframe extraction failed: {error}");
            }
        }
    }
    Ok((results, truncated))
}

async fn read_frame(page: &Page, frame: &Frame) -> Result<Option<(String, bool)>, String> {
    let params = CreateIsolatedWorldParams::builder()
        .frame_id(frame.id.clone())
        .world_name("n2-kraken-reader")
        .build()
        .map_err(|e| e.to_string())?;
    let world = page.execute(params).await.map_err(|e| e.to_string())?;
    let params = EvaluateParams::builder()
        .expression(EXTRACT_JS)
        .context_id(world.result.execution_context_id)
        .return_by_value(true)
        .build()
        .map_err(|e| e.to_string())?;
    let response = page.execute(params).await.map_err(|e| e.to_string())?;
    let Some(raw) = response
        .result
        .result
        .value
        .as_ref()
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok(parsed["content"]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(|text| {
            (
                text.to_owned(),
                parsed["truncated"].as_bool().unwrap_or(true),
            )
        }))
}
