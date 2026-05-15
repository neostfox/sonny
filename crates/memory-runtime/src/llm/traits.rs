use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::error::{MemoryError, MemoryResult};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> MemoryResult<String>;

    async fn complete_json<T: DeserializeOwned>(
        &self,
        prompt: &str,
        system: Option<&str>,
    ) -> MemoryResult<T> {
        let raw = self.complete(prompt, system).await?;
        serde_json::from_str(&raw).map_err(|e| MemoryError::LlmInvalidJson { source: e, raw })
    }

    async fn health_check(&self) -> MemoryResult<bool>;

    fn name(&self) -> &str;
}
