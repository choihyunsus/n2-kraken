// One versioned request contains a typed operation and an explicit correlation identifier.
use n2_kraken::{HtmlToMarkdownConfig, StudyConfig};
use serde::Deserialize;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u8,
    pub id: String,
    pub command: Command,
}

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    Extract(Extract),
    Fetch(Fetch),
    Render(Render),
    Search(Search),
    Research(Research),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extract {
    pub html: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub config: HtmlToMarkdownConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fetch {
    pub url: String,
    #[serde(default)]
    pub config: HtmlToMarkdownConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Render {
    pub url: String,
    #[serde(default)]
    pub format: RenderFormat,
    #[serde(default = "default_output_limit")]
    pub max_length: usize,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RenderFormat {
    #[default]
    Text,
    Html,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub query: String,
    #[serde(default)]
    pub config: StudyConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Research {
    pub topic: String,
    #[serde(default)]
    pub config: StudyConfig,
    #[serde(default)]
    pub semantic: bool,
}

fn default_output_limit() -> usize {
    50000
}

pub fn read_request(bytes: &[u8]) -> Result<Request, &'static str> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err("Native request exceeds its byte limit");
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "Invalid native request")?;
    if request.protocol != PROTOCOL_VERSION
        || request.id.is_empty()
        || request.id.len() > 128
        || request
            .id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("Invalid native protocol or request identifier");
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_and_operation_fields_are_checked_before_execution() {
        let valid = br#"{"protocol":1,"id":"fixture","command":{"operation":"extract","arguments":{"html":"<p>hello</p>"}}}"#;
        assert!(read_request(valid).is_ok());
        for invalid in [
            br#"{"protocol":2,"id":"fixture","command":{"operation":"extract","arguments":{"html":""}}}"#.as_slice(),
            br#"{"protocol":1,"id":"bad id","command":{"operation":"extract","arguments":{"html":""}}}"#,
            br#"{"protocol":1,"id":"fixture","extra":1,"command":{"operation":"extract","arguments":{"html":""}}}"#,
            br#"{"protocol":1,"id":"fixture","command":{"operation":"extract","arguments":{"html":"","extra":1}}}"#,
            br#"{"protocol":1,"id":"fixture","command":{"operation":"unsupported","arguments":{}}}"#,
        ] { assert!(read_request(invalid).is_err()); }
        assert!(read_request(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    }
}
