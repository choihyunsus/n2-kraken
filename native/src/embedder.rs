// n2-kraken: embedding trait + optional Ollama implementation
use async_trait::async_trait;

/// Trait for computing text embeddings. Consumers can implement for any backend.
#[async_trait]
pub trait Embedder: Send + Sync {
    fn is_available(&self) -> bool;
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

/// No-op embedder (always unavailable). Used when semantic mode is disabled.
pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    fn is_available(&self) -> bool {
        false
    }
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, String> {
        Err("Embedder not available".into())
    }
    async fn embed_batch(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Err("Embedder not available".into())
    }
}

#[cfg(feature = "ollama")]
#[path = "embedder_ollama.rs"]
mod ollama;
#[cfg(feature = "ollama")]
pub use ollama::OllamaEmbedder;
