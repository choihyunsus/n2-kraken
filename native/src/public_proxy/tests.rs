// Verify destination policy and raw proxy protocol separately from browser rendering.
use super::*;

#[test]
fn rejects_private_special_and_translation_destinations() {
    for ip in [
        "0.1.2.3",
        "10.0.0.1",
        "127.1.2.3",
        "169.254.169.254",
        "172.31.0.1",
        "192.168.1.1",
        "100.100.100.200",
        "192.0.0.8",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "::1",
        "::",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "64:ff9b::7f00:1",
        "2001:db8::1",
        "2002:7f00:1::",
        "3fff::1",
    ] {
        assert!(!egress::public_ip(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "1.1.1.1",
        "8.8.8.8",
        "93.184.215.14",
        "2001:4860:4860::8888",
        "2606:4700:4700::1111",
        "::ffff:8.8.8.8",
    ] {
        assert!(egress::public_ip(ip.parse().unwrap()), "{ip}");
    }
}

#[test]
fn headers_preserve_destination_and_remove_proxy_credentials() {
    let request = protocol::parse(b"GET http://example.com:8080/path?q=one HTTP/1.1\r\nHost: misleading.invalid\r\nProxy-Authorization: secret\r\nCookie: scoped=yes\r\nConnection: keep-alive\r\n\r\n").unwrap();
    let forwarded = String::from_utf8(request.forwarded).unwrap();
    assert_eq!(request.url.host_str(), Some("example.com"));
    assert!(forwarded.starts_with(
        "GET /path?q=one HTTP/1.1\r\nHost: example.com:8080\r\nConnection: close\r\n"
    ));
    assert!(forwarded.contains("Cookie: scoped=yes"));
    assert!(
        !forwarded.contains("secret")
            && !forwarded.contains("misleading")
            && !forwarded.contains("keep-alive")
    );
}

#[test]
fn rejects_ambiguous_headers_invalid_tunnels_and_schemes() {
    for value in [
        "CONNECT localhost:443/path HTTP/1.1\r\n\r\n",
        "CONNECT user@host:443 HTTP/1.1\r\n\r\n",
        "CONNECT example.com HTTP/1.1\r\n\r\n",
        "GET file:///secret HTTP/1.1\r\n\r\n",
        "GET http://user:pass@example.com/ HTTP/1.1\r\n\r\n",
        "GET http://example.com/ HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n",
        "POST http://example.com/ HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
        "POST http://example.com/ HTTP/1.1\r\nContent-Length: invalid\r\n\r\n",
    ] {
        assert!(protocol::parse(value.as_bytes()).is_err(), "{value}");
    }
    let tunnel =
        protocol::parse(b"CONNECT [2606:4700:4700::1111]:443 HTTP/1.1\r\nHost: ignored\r\n\r\n")
            .unwrap();
    assert!(tunnel.tunnel && tunnel.forwarded.is_empty());
}

#[tokio::test]
async fn actual_proxy_blocks_private_http_and_connect_before_opening_the_destination() {
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = local.local_addr().unwrap().port();
    let proxy = start().await.unwrap();
    for request in [
        format!("GET http://127.0.0.1:{port}/ HTTP/1.1\r\n\r\n"),
        format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n"),
        format!("GET http://[::ffff:127.0.0.1]:{port}/ HTTP/1.1\r\n\r\n"),
    ] {
        let mut stream = TcpStream::connect(proxy.address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 403 Forbidden"), "{reply}");
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), local.accept())
            .await
            .is_err()
    );
    proxy.task.abort();
}

#[tokio::test]
async fn incomplete_oversized_headers_and_transfer_limits_are_bounded() {
    let proxy = start().await.unwrap();
    let mut stream = TcpStream::connect(proxy.address).await.unwrap();
    stream
        .write_all(&vec![b'x'; protocol::MAX_HEADER + 1])
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 403"));
    proxy.task.abort();
    let mut exact = Vec::new();
    copy_limited(&b"12345678"[..], &mut exact, 8).await.unwrap();
    assert_eq!(exact, b"12345678");
    assert!(
        copy_limited(&b"123456789"[..], tokio::io::sink(), 8)
            .await
            .is_err()
    );
}

#[test]
fn a_mixed_dns_answer_is_rejected_as_a_whole() {
    let public = "1.1.1.1:443".parse().unwrap();
    let private = "127.0.0.1:443".parse().unwrap();
    assert!(egress::validate_addresses(&[public]).is_ok());
    assert!(egress::validate_addresses(&[public, private]).is_err());
    assert!(egress::validate_addresses(&[private, public]).is_err());
    assert!(egress::validate_addresses(&[]).is_err());
    assert!(egress::validate_addresses(&["1.1.1.1:0".parse().unwrap()]).is_err());
}

const TRANSPORT_CASES: &[(&str, &str, &str)] = &[
    (
        "POST http://example.com/path HTTP/1.1\r\nContent-Length: 4\r\n\r\n",
        "POST /path HTTP/1.1",
        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nREPLY",
    ),
    (
        "CONNECT example.com:443 HTTP/1.1\r\n\r\n",
        "BODY",
        "TUNNEL_BYTES",
    ),
    (
        "GET http://example.com/socket HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: fixture\r\n\r\n",
        "Upgrade: websocket",
        "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\nFRAME",
    ),
];

#[tokio::test]
async fn http_body_connect_and_websocket_bytes_use_the_selected_upstream() {
    for &(header, expected, reply) in TRANSPORT_CASES {
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ingress = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(ingress.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = ingress.accept().await.unwrap();
        // Explicit test-only connected transport; production always resolves and checks public IPs.
        let upstream = TcpStream::connect(local.local_addr().unwrap())
            .await
            .unwrap();
        let (mut server, _) = local.accept().await.unwrap();
        let request = protocol::parse(header.as_bytes()).unwrap();
        let tunnel = request.tunnel;
        let bridge = tokio::spawn(forward(accepted, upstream, request, b"BO".to_vec()));
        client.write_all(b"DY").await.unwrap();
        client.shutdown().await.unwrap();
        let mut received = String::new();
        server.read_to_string(&mut received).await.unwrap();
        assert!(
            received.contains(expected) && received.ends_with("BODY"),
            "{received}"
        );
        server.write_all(reply.as_bytes()).await.unwrap();
        server.shutdown().await.unwrap();
        let mut returned = String::new();
        client.read_to_string(&mut returned).await.unwrap();
        assert!(returned.ends_with(reply), "{returned}");
        if tunnel {
            assert!(returned.starts_with("HTTP/1.1 200 Connection Established"));
        }
        bridge.await.unwrap().unwrap();
    }
}
