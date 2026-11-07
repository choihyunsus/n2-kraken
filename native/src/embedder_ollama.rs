// Explicit operator-owned Ollama endpoint using the documented bounded batch embedding API.
use super::Embedder;
use async_trait::async_trait;
use futures::StreamExt;

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXTS: usize = 300;
const MAX_DIMENSIONS: usize = 8192;

pub struct OllamaEmbedder {
    endpoint: url::Url,
    model: String,
    client: wreq::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: &str, model: &str) -> Result<Self, String> {
        if base_url.len() > 8192
            || model.trim().is_empty()
            || model.len() > 256
            || model.chars().any(char::is_control)
        {
            return Err("Invalid embedding configuration".into());
        }
        let mut endpoint = url::Url::parse(base_url).map_err(|_| "Invalid embedding endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.port_or_known_default() == Some(0)
        {
            return Err("Invalid embedding endpoint".into());
        }
        endpoint.set_path(&format!(
            "{}/api/embed",
            endpoint.path().trim_end_matches('/')
        ));
        let client = wreq::Client::builder()
            .no_proxy()
            .redirect(wreq::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|_| "Embedding transport initialization failed")?;
        Ok(Self {
            endpoint,
            model: model.to_owned(),
            client,
        })
    }

    pub fn default_local() -> Result<Self, String> {
        Self::new(
            &std::env::var("OLLAMA_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
            &std::env::var("OLLAMA_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".into()),
        )
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn is_available(&self) -> bool {
        true
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed_batch(&[text.to_owned()])
            .await?
            .pop()
            .ok_or_else(|| "Missing embedding".into())
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if texts.len() > MAX_TEXTS
            || texts
                .iter()
                .any(|text| text.len() > 8192 || text.contains('\0'))
        {
            return Err("Embedding input exceeds limits".into());
        }
        let body = serde_json::to_vec(&serde_json::json!({
            "model": self.model, "input": texts, "truncate": false,
        }))
        .map_err(|_| "Invalid embedding input")?;
        if body.len() > 512 * 1024 {
            return Err("Embedding batch exceeds byte limit".into());
        }
        let response = self
            .client
            .post(self.endpoint.as_str())
            .header(wreq::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| "Embedding request failed")?;
        if !response.status().is_success() {
            return Err(format!("Embedding HTTP {}", response.status().as_u16()));
        }
        let bytes = read_body(response).await?;
        let parsed: EmbeddingResponse =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid embedding response")?;
        validate(&parsed.embeddings, texts.len())?;
        Ok(parsed.embeddings)
    }
}

#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    embeddings: Vec<Vec<f32>>,
}

async fn read_body(response: wreq::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BODY_BYTES as u64)
    {
        return Err("Embedding response exceeds byte limit".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Embedding response interrupted")?;
        if chunk.len() > MAX_BODY_BYTES.saturating_sub(bytes.len()) {
            return Err("Embedding response exceeds byte limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate(vectors: &[Vec<f32>], expected: usize) -> Result<(), String> {
    let dimensions = vectors.first().map_or(0, Vec::len);
    if vectors.len() != expected
        || dimensions == 0
        || dimensions > MAX_DIMENSIONS
        || vectors.iter().any(|vector| {
            vector.len() != dimensions
                || vector.iter().any(|number| !number.is_finite())
                || vector.iter().all(|number| *number == 0.0)
        })
    {
        return Err("Embedding vectors have invalid values or dimensions".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "embedder_ollama_tests.rs"]
mod tests;
