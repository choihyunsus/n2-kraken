// Iterative HTML traversal with independent DOM depth and UTF-8 output budgets.
use super::HtmlToMarkdownConfig;
use ego_tree::NodeRef;
use scraper::node::{Element, Node};

const MAX_DOM_DEPTH: usize = 200;
const MAX_REFERENCE_BYTES: usize = 8192;

enum Task<'a> {
    Visit(NodeRef<'a, Node>, usize, usize),
    Children(Option<NodeRef<'a, Node>>, usize, usize),
    List(Option<NodeRef<'a, Node>>, usize, usize, bool, usize),
    Append(String),
}

struct Writer<'a> {
    config: &'a HtmlToMarkdownConfig,
    markdown: String,
    truncated: bool,
    iframe_srcs: Vec<String>,
}

pub(super) fn render(
    node: NodeRef<'_, Node>,
    config: &HtmlToMarkdownConfig,
) -> (String, bool, Vec<String>) {
    let mut writer = Writer {
        config,
        markdown: String::new(),
        truncated: false,
        iframe_srcs: Vec::new(),
    };
    let mut stack = vec![Task::Visit(node, 0, 0)];
    while let Some(task) = stack.pop() {
        if writer.truncated {
            break;
        }
        match task {
            Task::Visit(node, level, depth) => writer.visit(node, level, depth, &mut stack),
            Task::Append(text) => writer.append(&text),
            Task::Children(Some(node), level, depth) => {
                stack.push(Task::Children(node.next_sibling(), level, depth));
                stack.push(Task::Visit(node, level, depth));
            }
            Task::List(Some(node), level, depth, ordered, index) => {
                writer.list_item(node, level, depth, ordered, index, &mut stack);
            }
            Task::Children(None, _, _) | Task::List(None, _, _, _, _) => {}
        }
    }
    (writer.markdown, writer.truncated, writer.iframe_srcs)
}

impl Writer<'_> {
    fn append(&mut self, text: &str) {
        let remaining = self.config.max_length.saturating_sub(self.markdown.len());
        let end = text.floor_char_boundary(remaining.min(text.len()));
        self.markdown.push_str(&text[..end]);
        if end < text.len() {
            self.truncated = true;
        }
    }

    fn visit<'a>(
        &mut self,
        node: NodeRef<'a, Node>,
        level: usize,
        depth: usize,
        stack: &mut Vec<Task<'a>>,
    ) {
        if depth > MAX_DOM_DEPTH {
            self.truncated = true;
            return;
        }
        match node.value() {
            Node::Text(text) => {
                let content = text.text.replace('\n', " ");
                let content = content.trim();
                if !content.is_empty() {
                    self.append(content);
                    self.append(" ");
                }
            }
            Node::Element(element) => self.element(node, element, level, depth, stack),
            _ => {}
        }
    }

    fn element<'a>(
        &mut self,
        node: NodeRef<'a, Node>,
        element: &Element,
        level: usize,
        depth: usize,
        stack: &mut Vec<Task<'a>>,
    ) {
        if hidden(element) {
            return;
        }
        let tag = element.name();
        if tag == "iframe" {
            self.iframe(element);
            return;
        }
        if skipped(tag) {
            return;
        }
        match tag {
            "br" => self.append("\n"),
            "hr" => self.append("\n---\n\n"),
            "ul" | "ol" => {
                stack.push(Task::Append("\n".into()));
                stack.push(Task::List(
                    node.first_child(),
                    level,
                    depth + 1,
                    tag == "ol",
                    1,
                ));
                self.append("\n\n");
            }
            "img" => self.image(element),
            "a" => {
                if !self.link(node, element) {
                    stack.push(Task::Children(node.first_child(), level + 1, depth + 1));
                }
            }
            _ => {
                if let Some((start, end)) = wrappers(tag) {
                    stack.push(Task::Append(end));
                    stack.push(Task::Children(node.first_child(), level + 1, depth + 1));
                    self.append(&start);
                } else {
                    stack.push(Task::Children(node.first_child(), level, depth + 1));
                }
            }
        }
    }

    fn iframe(&mut self, element: &Element) {
        if let Some(src) = element.attr("src").filter(|src| src.starts_with("http")) {
            if self.iframe_srcs.len() >= 32 || src.len() > MAX_REFERENCE_BYTES {
                self.truncated = true;
                return;
            }
            if !safe_reference(src) {
                return;
            }
            self.iframe_srcs.push(src.to_string());
            self.append(&format!("\n[Embedded Iframe: {src}]\n"));
        }
    }

    fn list_item<'a>(
        &mut self,
        node: NodeRef<'a, Node>,
        level: usize,
        depth: usize,
        ordered: bool,
        index: usize,
        stack: &mut Vec<Task<'a>>,
    ) {
        let is_item = matches!(node.value(), Node::Element(element) if element.name() == "li");
        stack.push(Task::List(
            node.next_sibling(),
            level,
            depth,
            ordered,
            index + usize::from(is_item),
        ));
        if !is_item {
            return;
        }
        if depth > MAX_DOM_DEPTH {
            self.truncated = true;
            return;
        }
        stack.push(Task::Append("\n".into()));
        stack.push(Task::Children(node.first_child(), level + 1, depth + 1));
        let prefix = if ordered {
            format!("{index}. ")
        } else {
            "- ".into()
        };
        self.append(&format!("{}{prefix}", "  ".repeat(level.saturating_sub(1))));
    }

    fn link(&mut self, node: NodeRef<'_, Node>, element: &Element) -> bool {
        if !self.config.include_links {
            return false;
        }
        let Some(href) = element.attr("href") else {
            return false;
        };
        if href.len() > MAX_REFERENCE_BYTES {
            self.truncated = true;
            return false;
        }
        if !safe_reference(href) {
            return false;
        }
        let mut content = String::new();
        for child in node.children() {
            if let Node::Text(text) = child.value() {
                content.push_str(&text.text);
            }
        }
        let content = content.trim();
        if content.is_empty() {
            return false;
        }
        self.append(&format!("[{content}]({href})"));
        true
    }

    fn image(&mut self, element: &Element) {
        if !self.config.include_images {
            return;
        }
        let src = element.attr("src").unwrap_or("");
        let alt = element.attr("alt").unwrap_or("");
        if src.len() > MAX_REFERENCE_BYTES {
            self.truncated = true;
            return;
        }
        if safe_reference(src) {
            self.append(&format!("![{alt}]({src})"));
        }
    }
}

fn safe_reference(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && url::Url::parse("https://document.invalid/")
            .and_then(|base| base.join(value))
            .is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none()
            })
}

fn hidden(element: &Element) -> bool {
    let classes = element.attr("class").unwrap_or("").to_lowercase();
    let style = element.attr("style").unwrap_or("").to_lowercase();
    [
        "display:none",
        "display: none",
        "visibility:hidden",
        "visibility: hidden",
    ]
    .iter()
    .any(|needle| style.contains(needle))
        || [
            "sr-only",
            "screen-reader",
            "hidden",
            "advertisement",
            "nav-",
            "sidebar",
            "footer",
            "cookie-banner",
            "promo",
        ]
        .iter()
        .any(|needle| classes.contains(needle))
}

fn skipped(tag: &str) -> bool {
    [
        "script", "style", "noscript", "svg", "nav", "footer", "header", "form", "aside", "button",
        "input", "object", "embed", "applet", "link", "meta", "iframe",
    ]
    .contains(&tag)
}

fn wrappers(tag: &str) -> Option<(String, String)> {
    let (start, end) = match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            return Some((
                format!(
                    "\n\n{} ",
                    "#".repeat(tag[1..].parse::<usize>().unwrap_or(1))
                ),
                "\n\n".into(),
            ));
        }
        "p" => ("\n\n", "\n\n"),
        "strong" | "b" => ("**", "**"),
        "em" | "i" => ("*", "*"),
        "code" => ("`", "`"),
        "pre" => ("\n```\n", "\n```\n\n"),
        "table" => ("\n--- Table Data ---\n", "\n------------------\n"),
        "blockquote" => ("\n> ", "\n\n"),
        _ => return None,
    };
    Some((start.into(), end.into()))
}
