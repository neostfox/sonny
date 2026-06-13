use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use memory_runtime::error::MemoryResult;
use memory_runtime::llm::traits::LlmProvider;

pub struct MockLlmProvider {
    responses: HashMap<String, String>,
    default_response: String,
    pub call_log: Mutex<Vec<String>>,
}

impl MockLlmProvider {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            default_response: r#"{"observations":[]}"#.to_string(),
            call_log: Mutex::new(Vec::new()),
        }
    }

    pub fn with_response(mut self, pattern: &str, response: &str) -> Self {
        self.responses
            .insert(pattern.to_string(), response.to_string());
        self
    }

    pub fn set_default_response(&mut self, response: &str) {
        self.default_response = response.to_string();
    }
}

impl Default for MockLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete(&self, prompt: &str, _system: Option<&str>) -> MemoryResult<String> {
        self.call_log.lock().unwrap().push(prompt.to_string());

        for (pattern, response) in &self.responses {
            if prompt.contains(pattern) {
                return Ok(response.clone());
            }
        }

        Ok(self.default_response.clone())
    }

    async fn health_check(&self) -> MemoryResult<bool> {
        Ok(true)
    }

    fn name(&self) -> &str {
        "mock"
    }
}
