// Exercise bounded Ollama transport and embedding validation with isolated HTTP fixtures.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn server(response: String) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(socket.read_u8().await.unwrap());
            assert!(bytes.len() <= 16384);
        }
        let header = String::from_utf8(bytes.clone()).unwrap();
        let length: usize = header
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        assert!(length <= 512 * 1024);
        let mut body = vec![0; length];
        socket.read_exact(&mut body).await.unwrap();
        bytes.extend(body);
        socket.write_all(response.as_bytes()).await.unwrap();
        bytes
    });
    (format!("http://{address}"), task)
}

fn response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn one_batch_preserves_all_inputs_and_uses_explicit_nontruncating_embedding_api() {
    let (url, task) = server(response(r#"{"embeddings":[[1,0],[0,1]]}"#)).await;
    let embedder = OllamaEmbedder::new(&url, "fixture-model").unwrap();
    let texts = vec!["first 한글".into(), "second text".into()];
    assert_eq!(
        embedder.embed_batch(&texts).await.unwrap(),
        vec![vec![1.0, 0.0], vec![0.0, 1.0]]
    );
    let bytes = task.await.unwrap();
    let request = String::from_utf8(bytes).unwrap();
    assert!(request.starts_with("POST /api/embed HTTP/1.1\r\n"));
    let body: serde_json::Value =
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["input"], serde_json::json!(texts));
    assert_eq!(body["truncate"], false);
    assert_eq!(body["model"], "fixture-model");
}

#[tokio::test]
async fn malformed_partial_oversized_or_redirected_embedding_responses_fail_explicitly() {
    for raw in [
        response(r#"{"embeddings":[[1,"bad",0]]}"#),
        response(r#"{"embeddings":[[1e50,0]]}"#),
        response(r#"{"embeddings":[[0,0]]}"#),
        response(r#"{"embeddings":[]}"#),
        response(r#"{"embeddings":[[1,0],[1]]}"#),
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_BODY_BYTES + 1),
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    ] {
        let (url, task) = server(raw).await;
        assert!(OllamaEmbedder::new(&url, "fixture").unwrap().embed("test").await.is_err());
        task.await.unwrap();
    }
    assert!(validate(&[vec![1.0; MAX_DIMENSIONS + 1]], 1).is_err());
}

#[tokio::test]
async fn invalid_configuration_and_input_limits_fail_before_any_connection() {
    for url in [
        "file:///private",
        "http://user:secret@localhost/",
        "http://localhost/?key=value",
        "http://localhost/#fragment",
    ] {
        assert!(OllamaEmbedder::new(url, "fixture").is_err());
    }
    let embedder = OllamaEmbedder::new("http://127.0.0.1:1", "fixture").unwrap();
    assert!(embedder.embed_batch(&[]).await.unwrap().is_empty());
    assert!(
        embedder
            .embed_batch(&vec!["text".into(); MAX_TEXTS + 1])
            .await
            .is_err()
    );
    assert!(embedder.embed(&"a".repeat(8193)).await.is_err());
}
