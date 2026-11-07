// Fetch the explicitly configured search service with bounded bodies and redacted diagnostics.
use super::error::KrakenError;
use futures::StreamExt;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub(super) async fn fetch(
    base: &str,
    query: &str,
    time_range: &str,
) -> Result<String, KrakenError> {
    let configured = std::env::var("SEARXNG_URL")
        .map_err(|_| error("SearXNG requires explicit SEARXNG_URL configuration"))?;
    let url = endpoint(base, &configured, query, time_range)?;
    let client = wreq::Client::builder()
        .no_proxy()
        .redirect(wreq::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| error("Search service transport initialization failed"))?;
    let response = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|_| error("Search service request failed"))?;
    if !response.status().is_success() {
        return Err(error(&format!(
            "Search service HTTP {}",
            response.status().as_u16()
        )));
    }
    read_response(response).await
}

fn endpoint(
    base: &str,
    configured: &str,
    query: &str,
    time_range: &str,
) -> Result<url::Url, KrakenError> {
    if base.len() > 8192 || configured.len() > 8192 {
        return Err(error("Search service URL exceeds limits"));
    }
    let mut url = url::Url::parse(base).map_err(|_| error("Invalid search service URL"))?;
    let trusted =
        url::Url::parse(configured).map_err(|_| error("Invalid configured search service URL"))?;
    if url != trusted
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() == Some(0)
    {
        return Err(error(
            "Search service must match the configured HTTP(S) base URL",
        ));
    }
    url.set_path(&format!("{}/search", url.path().trim_end_matches('/')));
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "json")
        .append_pair("categories", "general")
        .append_pair("language", "en")
        .append_pair("pageno", "1");
    let range = match time_range {
        "1w" => "week",
        "1m" => "month",
        "3m" | "6m" | "1y" => "year",
        _ => "",
    };
    if !range.is_empty() {
        url.query_pairs_mut().append_pair("time_range", range);
    }
    Ok(url)
}

async fn read_response(response: wreq::Response) -> Result<String, KrakenError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(error("Search service response exceeds byte limit"));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| error("Search service response interrupted"))?;
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(error("Search service response exceeds byte limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| error("Search service response has invalid UTF-8"))
}

fn error(message: &str) -> KrakenError {
    KrakenError::Parse(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_configured_service_is_used_and_query_text_cannot_change_its_origin() {
        let base = "http://127.0.0.1:8080/prefix";
        let url = endpoint(base, base, "한글 &url=https://other.invalid", "3m").unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.path(), "/prefix/search");
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "q").unwrap().1,
            "한글 &url=https://other.invalid"
        );
        assert!(endpoint("http://127.0.0.1:9090", base, "test", "").is_err());
        for invalid in [
            "file:///private",
            "http://name:secret@localhost/",
            "http://localhost/?key=secret",
            "http://localhost/#fragment",
        ] {
            assert!(endpoint(invalid, invalid, "test", "").is_err());
        }
    }
}
