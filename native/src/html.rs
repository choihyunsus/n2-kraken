// n2-kraken: HTML-to-Markdown converter with DCR phantom filter and SGW boilerplate elimination
use scraper::Html;
#[path = "html_walk.rs"]
mod walk;
use std::sync::LazyLock;
const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
const MAX_MARKDOWN_BYTES: usize = 1024 * 1024;

static SEL_TITLE: LazyLock<scraper::Selector> =
    LazyLock::new(|| scraper::Selector::parse("title").unwrap());
static SEL_ARTICLE: LazyLock<scraper::Selector> =
    LazyLock::new(|| scraper::Selector::parse("article").unwrap());
static SEL_MAIN: LazyLock<scraper::Selector> =
    LazyLock::new(|| scraper::Selector::parse("main").unwrap());
static SEL_BODY: LazyLock<scraper::Selector> =
    LazyLock::new(|| scraper::Selector::parse("body").unwrap());

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HtmlToMarkdownConfig {
    pub include_links: bool,
    pub include_images: bool,
    pub max_length: usize,
}

impl Default for HtmlToMarkdownConfig {
    fn default() -> Self {
        Self {
            include_links: true,
            include_images: false,
            max_length: 50_000,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedDocument {
    pub title: String,
    pub source_url: String,
    pub markdown: String,
    pub truncated: bool,
    pub iframe_srcs: Vec<String>,
}

/// Core function to convert raw HTML string into clean, RAG-optimized Markdown
pub fn parse_html_to_markdown(
    html_content: &str,
    config: &HtmlToMarkdownConfig,
) -> Result<ExtractedDocument, super::error::KrakenError> {
    if html_content.len() > MAX_HTML_BYTES || config.max_length > MAX_MARKDOWN_BYTES {
        return Err(super::error::KrakenError::InvalidInput(
            "HTML input or Markdown output limit exceeded".into(),
        ));
    }
    let document = Html::parse_document(html_content);

    let (title, title_truncated) = document
        .select(&SEL_TITLE)
        .next()
        .map(bounded_title)
        .unwrap_or_default();

    let root_node = document
        .select(&SEL_ARTICLE)
        .next()
        .map(|er| er.id())
        .or_else(|| document.select(&SEL_MAIN).next().map(|er| er.id()))
        .or_else(|| document.select(&SEL_BODY).next().map(|er| er.id()));

    let mut markdown = String::new();
    let mut is_truncated = false;
    let mut iframe_srcs = Vec::new();

    if let Some(node) = root_node.and_then(|id| document.tree.get(id)) {
        (markdown, is_truncated, iframe_srcs) = walk::render(node, config);
    }

    let markdown = clean_markdown(&markdown);

    Ok(ExtractedDocument {
        title,
        source_url: String::new(),
        markdown,
        truncated: is_truncated || title_truncated,
        iframe_srcs,
    })
}

fn bounded_title(element: scraper::ElementRef<'_>) -> (String, bool) {
    let mut characters = element
        .text()
        .flat_map(|text| text.chars().chain(std::iter::once(' ')));
    let preview: String = characters.by_ref().take(8193).collect();
    let mut title = preview.trim().to_owned();
    let omitted = characters.next().is_some() || title.len() > 8192;
    title.truncate(title.floor_char_boundary(8192.min(title.len())));
    (title, omitted)
}

fn clean_markdown(md: &str) -> String {
    let mut cleaned = String::with_capacity(md.len());
    let mut newlines = 0;
    for c in md.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                cleaned.push(c);
            }
        } else {
            newlines = 0;
            cleaned.push(c);
        }
    }
    cleaned.trim().to_string()
}

#[cfg(test)]
#[path = "html_tests.rs"]
mod tests;
