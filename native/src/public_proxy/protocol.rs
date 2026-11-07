// Strict bounded proxy headers; the upstream TCP destination never comes from Host alone.
use super::egress::{denied, parse_url};
use std::io;
pub(super) const MAX_HEADER: usize = 16_384;
pub(super) struct Request {
    pub url: url::Url,
    pub tunnel: bool,
    pub forwarded: Vec<u8>,
}

fn target(method: &str, target: &str) -> io::Result<(url::Url, bool)> {
    if method == "CONNECT" {
        if target.contains(['/', '?', '#', '@']) {
            return Err(denied());
        }
        let url = parse_url(&format!("https://{target}"))?;
        if !target.ends_with(&format!(
            ":{}",
            url.port_or_known_default().ok_or_else(denied)?
        )) {
            return Err(denied());
        }
        return Ok((url, true));
    }
    if !matches!(
        method,
        "GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS"
    ) {
        return Err(denied());
    }
    let url = parse_url(target)?;
    if url.scheme() != "http" || url.fragment().is_some() {
        return Err(denied());
    }
    Ok((url, false))
}

pub(super) fn parse(bytes: &[u8]) -> io::Result<Request> {
    if bytes.len() > MAX_HEADER {
        return Err(denied());
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    if !request.parse(bytes).map_err(|_| denied())?.is_complete() || request.version != Some(1) {
        return Err(denied());
    }
    let method = request.method.ok_or_else(denied)?;
    let (url, tunnel) = target(method, request.path.ok_or_else(denied)?)?;
    let websocket = validate_headers(request.headers, method, tunnel)?;
    let forwarded = if tunnel {
        Vec::new()
    } else {
        forward_headers(method, &url, request.headers, websocket)
    };
    Ok(Request {
        url,
        tunnel,
        forwarded,
    })
}

fn validate_headers(
    headers: &[httparse::Header<'_>],
    method: &str,
    tunnel: bool,
) -> io::Result<bool> {
    let mut content_lengths = 0;
    let mut transfer_encodings = 0;
    let mut hosts = 0;
    for header in headers {
        match header.name.to_ascii_lowercase().as_str() {
            "content-length" => {
                content_lengths += 1;
                std::str::from_utf8(header.value)
                    .map_err(|_| denied())?
                    .parse::<u64>()
                    .map_err(|_| denied())?;
            }
            "transfer-encoding" => {
                transfer_encodings += 1;
                if !header.value.eq_ignore_ascii_case(b"chunked") {
                    return Err(denied());
                }
            }
            "host" => hosts += 1,
            _ => {}
        }
    }
    if hosts > 1
        || content_lengths > 1
        || transfer_encodings > 1
        || (content_lengths > 0 && transfer_encodings > 0)
    {
        return Err(denied());
    }
    if tunnel {
        return Ok(false);
    }
    let websocket = headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("upgrade")
            && header.value.eq_ignore_ascii_case(b"websocket")
    });
    if websocket && (method != "GET" || content_lengths > 0 || transfer_encodings > 0) {
        return Err(denied());
    }
    Ok(websocket)
}

fn forward_headers(
    method: &str,
    url: &url::Url,
    headers: &[httparse::Header<'_>],
    websocket: bool,
) -> Vec<u8> {
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let connection = if websocket {
        "Connection: Upgrade\r\nUpgrade: websocket"
    } else {
        "Connection: close"
    };
    let mut forwarded =
        format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\n{connection}\r\n").into_bytes();
    for header in headers {
        if [
            "host",
            "connection",
            "proxy-connection",
            "proxy-authorization",
            "upgrade",
        ]
        .iter()
        .any(|name| header.name.eq_ignore_ascii_case(name))
        {
            continue;
        }
        forwarded.extend_from_slice(header.name.as_bytes());
        forwarded.extend_from_slice(b": ");
        forwarded.extend_from_slice(header.value);
        forwarded.extend_from_slice(b"\r\n");
    }
    forwarded.extend_from_slice(b"\r\n");
    forwarded
}
