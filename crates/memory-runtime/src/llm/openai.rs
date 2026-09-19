//! OpenAI-compatible chat completion client for extract / feedback CLI paths.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{MemoryError, MemoryResult};
use crate::llm::traits::LlmProvider;

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

pub struct OpenAiCompatibleLlmProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleLlmProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        timeout: Duration,
    ) -> MemoryResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| MemoryError::Embedding(format!("llm http client: {e}")))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleLlmProvider {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> MemoryResult<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(ChatMessage {
                role: "system",
                content: sys,
            });
        }
        messages.push(ChatMessage {
            role: "user",
            content: prompt,
        });
        let body = ChatRequest {
            model: &self.model,
            messages,
            temperature: 0.2,
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| MemoryError::Embedding(format!("llm request failed: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MemoryError::Embedding(format!(
                "llm http {status}: {}",
                text.chars().take(300).collect::<String>()
            )));
        }
        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| MemoryError::Embedding(format!("llm decode: {e}")))?;
        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| MemoryError::Embedding("llm empty completion".into()))
    }

    async fn health_check(&self) -> MemoryResult<bool> {
        Ok(!self.api_key.is_empty())
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }
}
