// Transport regressions use private client construction; public fetch still rejects loopback.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn fixture(
    response: Vec<u8>,
) -> (
    String,
    std::net::SocketAddr,
    tokio::task::JoinHandle<String>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let byte = socket.read_u8().await.unwrap();
            request.push(byte);
            assert!(request.len() < 16_384);
        }
        socket.write_all(&response).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (
        format!("http://fixture.invalid:{}/", address.port()),
        address,
        task,
    )
}

async fn response(raw: &[u8]) -> wreq::Response {
    let (url, address, server) = fixture(raw.to_vec()).await;
    let fetcher = KrakenFetcher::new();
    let response = fetcher
        .build_pinned_client("fixture.invalid", &[address])
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(request.contains("Chrome/131."));
    assert!(!request.contains("CONSENT="));
    response
}

#[tokio::test]
async fn pinned_dns_preserves_host_port_and_charset() {
    let response = response(b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=windows-1252\r\nContent-Length: 4\r\nConnection: close\r\n\r\ncaf\xe9").await;
    assert_eq!(read_response(response, 20).await.unwrap(), "café");
}

#[tokio::test]
async fn http_errors_and_non_content_status_are_not_successful_pages() {
    for status in [
        "404 Not Found",
        "500 Internal Server Error",
        "304 Not Modified",
    ] {
        let raw = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(
            read_response(response(raw.as_bytes()).await, 20)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn chunked_and_declared_oversized_bodies_fail_before_unbounded_collection() {
    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n4\r\n9012\r\n0\r\n\r\n";
    assert!(matches!(
        read_response(response(chunked).await, 10).await,
        Err(FetchError::TooLarge(_))
    ));
    let declared = b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n";
    assert!(matches!(
        read_response(response(declared).await, 10).await,
        Err(FetchError::TooLarge(_))
    ));
}

#[tokio::test]
async fn redirects_are_not_automatically_followed() {
    let raw = b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    assert_eq!(response(raw).await.status().as_u16(), 302);
    let fetcher = KrakenFetcher::new();
    for url in [
        "http://127.0.0.1:1/private",
        "http://user:secret@example.com/",
        "file:///private",
    ] {
        assert!(matches!(
            fetcher.validated_client(url).await,
            Err(FetchError::SsrfBlocked(_))
        ));
    }
}

#[tokio::test]
async fn cookies_survive_pinned_client_rebuild_without_cross_domain_consent() {
    let fetcher = KrakenFetcher::new();
    let (first, address, server) = fixture(b"HTTP/1.1 200 OK\r\nSet-Cookie: session=test-value; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()).await;
    let client = fetcher
        .build_pinned_client("fixture.invalid", &[address])
        .unwrap();
    client.get(first).send().await.unwrap();
    server.await.unwrap();
    let (next, address, server) =
        fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec())
            .await;
    fetcher
        .build_pinned_client("fixture.invalid", &[address])
        .unwrap()
        .get(next)
        .send()
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(request.contains("session=test-value"));
    assert!(!request.contains("CONSENT="));
}

#[tokio::test]
async fn decompressed_response_is_bounded_and_supported_encoding_is_advertised() {
    let compressed: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 2, 10, 115, 116, 164, 61, 0, 0, 141, 188, 151, 149, 100, 0, 0, 0,
    ];
    let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", compressed.len()).into_bytes();
    raw.extend_from_slice(compressed);
    assert!(matches!(
        read_response(response(&raw).await, 50).await,
        Err(FetchError::TooLarge(_))
    ));
}
