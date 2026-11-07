// Operations return bounded source data; durable scope and knowledge storage belong to the host.
use crate::worker_protocol::{Command, Render, RenderFormat, Research};
use n2_kraken::{KrakenFetcher, browser::HeadlessBrowser, parse_html_to_markdown};
use serde_json::{Value, json};
const MAX_OUTPUT_BYTES: usize = 1_048_576;

pub async fn execute(command: Command) -> Result<Value, String> {
    match command {
        Command::Extract(input) => {
            let source_url = input
                .source_url
                .as_deref()
                .map(source_url)
                .transpose()?
                .unwrap_or_default();
            let mut result = parse_html_to_markdown(&input.html, &input.config)
                .map_err(|error| error.to_string())?;
            result.source_url = source_url;
            encode(result)
        }
        Command::Fetch(input) => {
            if input.config.max_length > MAX_OUTPUT_BYTES {
                return Err("Output limit exceeded".into());
            }
            let fetched = KrakenFetcher::new()
                .fetch_document(&input.url)
                .await
                .map_err(|error| error.to_string())?;
            let mut result = parse_html_to_markdown(&fetched.html, &input.config)
                .map_err(|error| error.to_string())?;
            result.source_url = fetched.url;
            encode(result)
        }
        Command::Render(input) => render(input).await,
        Command::Search(input) => encode(
            n2_kraken::search::search(&input.query, &input.config, &KrakenFetcher::new())
                .await
                .map_err(|error| error.to_string())?,
        ),
        Command::Research(input) => research(input).await,
    }
}

async fn render(input: Render) -> Result<Value, String> {
    if input.max_length > MAX_OUTPUT_BYTES {
        return Err("Rendered output limit exceeded".into());
    }
    KrakenFetcher::check_ssrf(&input.url)
        .await
        .map_err(|error| error.to_string())?;
    let browser = HeadlessBrowser::global()
        .await
        .map_err(|_| "Headless Chrome unavailable; check its installation and CHROME_PATH")?;
    let (url, title, content, iframe_count, format, omitted) = match input.format {
        RenderFormat::Html => {
            let page = browser
                .fetch_document(&input.url)
                .await
                .map_err(|_| "Rendered HTML retrieval failed")?;
            (page.url, String::new(), page.html, 0, "html", false)
        }
        RenderFormat::Text => {
            let page = browser
                .fetch_cleaned(&input.url)
                .await
                .map_err(|_| "Rendered text retrieval failed")?;
            (
                source_url(&page.url)?,
                page.title,
                page.content,
                page.iframe_count,
                "text",
                page.truncated,
            )
        }
    };
    let truncated = omitted || content.len() > input.max_length;
    let end = content.floor_char_boundary(input.max_length.min(content.len()));
    Ok(
        json!({"url":url,"title":title,"content":&content[..end],"iframe_count":iframe_count,
        "format":format,"truncated":truncated}),
    )
}

async fn research(input: Research) -> Result<Value, String> {
    let fetcher = KrakenFetcher::new();
    let embedder = configured_embedder(input.semantic)?;
    let report = n2_kraken::research(&input.topic, &input.config, &fetcher, embedder.as_deref())
        .await
        .map_err(|error| error.to_string())?;
    encode(report)
}

fn configured_embedder(
    enabled: bool,
) -> Result<Option<Box<dyn n2_kraken::embedder::Embedder>>, String> {
    if !enabled {
        return Ok(None);
    }
    #[cfg(feature = "ollama")]
    {
        Ok(Some(Box::new(
            n2_kraken::embedder::OllamaEmbedder::default_local()?,
        )))
    }
    #[cfg(not(feature = "ollama"))]
    {
        Err("This worker was built without Ollama embedding support".into())
    }
}

fn source_url(value: &str) -> Result<String, String> {
    if value.len() > 8192 || value.chars().any(char::is_control) {
        return Err("Invalid source URL".into());
    }
    let mut parsed = url::Url::parse(value).map_err(|_| "Invalid source URL")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() == Some(0)
    {
        return Err("Invalid source URL".into());
    }
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}

fn encode(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "Native result serialization failed".into())
}
