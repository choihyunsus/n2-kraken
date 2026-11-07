// Regression tests for bounded traversal and exact output limits.
use super::*;
const SMALL_OUTPUT_LIMIT: usize = 128;
const NORMAL_OUTPUT_LIMIT: usize = 50000;

#[test]
fn metadata_and_input_limits_remain_bounded() {
    let source = format!(
        "<title>{}</title><main>{}</main>",
        "한글😀".repeat(5000),
        "<iframe src='https://example.com/frame'></iframe>".repeat(40)
    );
    let doc = parse_html_to_markdown(&source, &config(100000)).unwrap();
    assert!(doc.truncated && doc.title.len() <= 8192 && doc.iframe_srcs.len() <= 32);
    assert!(
        parse_html_to_markdown(&"a".repeat(MAX_HTML_BYTES + 1), &config(SMALL_OUTPUT_LIMIT))
            .is_err()
    );
    assert!(parse_html_to_markdown("text", &config(MAX_MARKDOWN_BYTES + 1)).is_err());
}

#[test]
fn active_and_credential_bearing_links_are_reduced_to_their_visible_text() {
    let source = "<main><a href='JaVaScRiPt:alert(1)'>Visible label</a><a href='data:text/html,payload'>Data label</a><img src='file:///private' alt='private'><a href='//name:secret@example.com/'>Credential label</a><a href='../guide'>Relative guide</a></main>";
    let doc = parse_html_to_markdown(source, &config(NORMAL_OUTPUT_LIMIT)).unwrap();
    assert!(doc.markdown.contains("Visible label") && doc.markdown.contains("Data label"));
    assert!(
        !doc.markdown.contains("alert(1)")
            && !doc.markdown.contains("secret")
            && !doc.markdown.contains("file:")
    );
    assert!(doc.markdown.contains("[Relative guide](../guide)"));
}

fn config(limit: usize) -> HtmlToMarkdownConfig {
    HtmlToMarkdownConfig {
        include_links: true,
        include_images: true,
        max_length: limit,
    }
}

#[test]
fn a_single_unicode_text_node_obeys_byte_budget() {
    let source = format!("<main>{}</main>", "한글🙂".repeat(2000));
    let result = parse_html_to_markdown(&source, &config(127)).unwrap();
    assert!(result.truncated && result.markdown.len() <= 127);
    assert!(result.markdown.chars().all(|c| "한글🙂".contains(c)));
}

#[test]
fn zero_budget_returns_no_content_and_marks_omission() {
    let result = parse_html_to_markdown("<main>VISIBLE</main>", &config(0)).unwrap();
    assert!(result.truncated && result.markdown.is_empty());
}

#[test]
fn links_and_images_cannot_bypass_output_budget() {
    for tag in [
        format!(
            "<a href='https://example.invalid/{}'>source</a>",
            "a".repeat(16000)
        ),
        format!(
            "<img src='https://example.invalid/{}' alt='photo'>",
            "a".repeat(16000)
        ),
    ] {
        let result =
            parse_html_to_markdown(&format!("<main>{tag}</main>"), &config(SMALL_OUTPUT_LIMIT))
                .unwrap();
        assert!(result.truncated && result.markdown.len() <= SMALL_OUTPUT_LIMIT);
    }
}

#[test]
fn ordinary_container_depth_is_bounded_and_reported() {
    let source = format!(
        "<main>{}DEEP_CONTENT{}</main>",
        "<div>".repeat(512),
        "</div>".repeat(512)
    );
    let result = parse_html_to_markdown(&source, &config(NORMAL_OUTPUT_LIMIT)).unwrap();
    assert!(result.truncated && !result.markdown.contains("DEEP_CONTENT"));
}

#[test]
fn ordinary_content_keeps_title_links_lists_and_noise_filter() {
    let source = "<title>Owned title</title><main><h1>Heading</h1><p>Hello <strong>world</strong></p><a href='https://example.invalid/source'>Source</a><ol><li>One</li><li>Two</li></ol><div style='display:none'>HIDDEN</div><nav>NAV</nav></main>";
    let result = parse_html_to_markdown(source, &config(NORMAL_OUTPUT_LIMIT)).unwrap();
    assert_eq!(result.title, "Owned title");
    assert!(!result.truncated && result.markdown.contains("# Heading"));
    assert!(
        result
            .markdown
            .contains("[Source](https://example.invalid/source)")
    );
    assert!(result.markdown.contains("1. One") && result.markdown.contains("2. Two"));
    assert!(!result.markdown.contains("HIDDEN") && !result.markdown.contains("NAV"));
}
