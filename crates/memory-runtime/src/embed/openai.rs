//! OpenAI-compatible embedding service — a single thin HTTP client that talks the
//! `/v1/embeddings` protocol. Covers OpenAI cloud and any compatible local server
//! (Ollama / Xinference / TEI / vLLM / LM Studio). Reference patterns: async-openai
//! `embedding.rs` (request/response shape), rig `openai::Client::from_url` (base_url config).
//!
//! Config-driven: base_url / api_key / model / dim come from `EmbeddingConfig`, falling back
//! to the LLM endpoint when the embedding-specific fields are empty (same provider serves both).

use std::time::Duration;

use async_trait::async_trait;
use ndarray::Array1;
use serde::{Deserialize, Serialize};

use crate::error::{MemoryError, MemoryResult};

use super::traits::EmbeddingProvider;

/// `/v1/embeddings` request body.
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

/// `/v1/embeddings` response body (only the fields we consume).
#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

pub struct OpenAiCompatibleEmbeddingProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dim: usize,
}

impl OpenAiCompatibleEmbeddingProvider {
    /// Build a provider pointing at `{base_url}/embeddings`. `base_url` may include a trailing
    /// `/v1` (OpenAI cloud) or be a bare local origin (`http://localhost:11434/v1` for Ollama).
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        dim: usize,
        timeout: Duration,
    ) -> MemoryResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| MemoryError::Embedding(format!("http client build failed: {e}")))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dim,
        })
    }

    /// POST one batch of inputs and parse the response, validating each vector's length.
    async fn embed_inner(&self, inputs: &[&str]) -> MemoryResult<Vec<Array1<f32>>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbeddingRequest {
            model: &self.model,
            input: inputs.to_vec(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| MemoryError::Embedding(format!("embeddings request failed: {e}")))?;
        let status = resp.status();
        let parsed: EmbeddingResponse = resp.json().await.map_err(|e| {
            MemoryError::Embedding(format!("embeddings parse failed ({status}): {e}"))
        })?;
        if parsed.data.len() != inputs.len() {
            return Err(MemoryError::Embedding(format!(
                "embeddings count mismatch: requested {}, got {}",
                inputs.len(),
                parsed.data.len()
            )));
        }
        parsed
            .data
            .into_iter()
            .map(|d| {
                if d.embedding.len() != self.dim {
                    return Err(MemoryError::Embedding(format!(
                        "embedding dim mismatch: model '{}' returned {}, configured {}",
                        self.model,
                        d.embedding.len(),
                        self.dim
                    )));
                }
                Ok(Array1::from(d.embedding))
            })
            .collect()
    }

    #[cfg(test)]
    /// Parse a canned response without any network — used by health_check and unit tests.
    fn parse_response(raw: &str, expected_dim: usize) -> MemoryResult<Vec<Array1<f32>>> {
        let parsed: EmbeddingResponse = serde_json::from_str(raw)
            .map_err(|e| MemoryError::Embedding(format!("embeddings parse failed: {e}")))?;
        parsed
            .data
            .into_iter()
            .map(|d| {
                if d.embedding.len() != expected_dim {
                    return Err(MemoryError::Embedding(format!(
                        "embedding dim mismatch: got {}, expected {}",
                        d.embedding.len(),
                        expected_dim
                    )));
                }
                Ok(Array1::from(d.embedding))
            })
            .collect()
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    async fn embed(&self, text: &str) -> MemoryResult<Array1<f32>> {
        let mut v = self.embed_inner(&[text]).await?;
        Ok(v.remove(0))
    }

    async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Array1<f32>>> {
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.embed_inner(&refs).await
    }

    fn dim(&self) -> usize {
        self.dim
    }

    async fn health_check(&self) -> MemoryResult<bool> {
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbeddingRequest {
            model: &self.model,
            input: vec!["ping"],
        };
        match self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_model_and_input() {
        let req = EmbeddingRequest {
            model: "BAAI/bge-m3",
            input: vec!["hello", "world"],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""model":"BAAI/bge-m3""#));
        assert!(json.contains(r#""input":["hello","world"]"#));
    }

    #[test]
    fn response_parses_to_vectors() {
        let dim = 4;
        let raw = r#"{"data":[{"embedding":[0.1,0.2,0.3,0.4]},{"embedding":[0.5,0.6,0.7,0.8]}]}"#;
        let vecs = OpenAiCompatibleEmbeddingProvider::parse_response(raw, dim).unwrap();
        assert_eq!(vecs.len(), 2);
        assert_eq!(vecs[0].len(), dim);
        assert!((vecs[0][0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn dim_mismatch_is_rejected() {
        let raw = r#"{"data":[{"embedding":[0.1,0.2,0.3,0.4]}]}"#;
        match OpenAiCompatibleEmbeddingProvider::parse_response(raw, 1024).unwrap_err() {
            MemoryError::Embedding(msg) => assert!(msg.contains("dim mismatch")),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn constructor_strips_trailing_slash() {
        let svc = OpenAiCompatibleEmbeddingProvider::new(
            "http://localhost:11434/v1/",
            "k",
            "bge-m3",
            1024,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(svc.base_url, "http://localhost:11434/v1");
        assert_eq!(svc.dim(), 1024);
        assert_eq!(svc.name(), "openai-compatible");
    }
}
