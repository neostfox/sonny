use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub model_dir: PathBuf,
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    pub clustering: ClusterConfig,
    pub recall: RecallConfig,
    pub confidence: ConfidenceConfig,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Optional embedding-specific OpenAI-compatible API base URL. Empty means: fall
    /// back to `LlmConfig.api_url` (same provider/server serves chat + embeddings).
    pub api_url: String,
    /// Optional embedding-specific API key. Empty means: fall back to `LlmConfig.api_key`.
    pub api_key: String,
    /// Embedding model name sent to `/v1/embeddings`.
    pub model_id: String,
    /// Vector dimension. Must match the model; changing it requires full re-embed.
    pub dim: usize,
    pub timeout_secs: u64,
    pub prefer_vec: bool,
}

#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub hac_threshold: f64,
    pub entity_merge_threshold: f64,
    pub incremental_max: usize,
}

#[derive(Debug, Clone)]
pub struct RecallConfig {
    pub semantic_weight: f32,
    pub entity_weight: f32,
    pub top_k: usize,
    pub top_n: usize,
    pub max_context_tokens: usize,
    pub semantic_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct ConfidenceConfig {
    pub auto_confirm_vitality: f32,
    pub auto_confirm_sessions: usize,
    pub auto_demote_vitality: f32,
    pub auto_demote_age_days: u32,
    pub decay_half_life_days: f32,
}

impl Settings {
    pub fn load() -> Self {
        let data_dir = std::env::var("SONNY_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".sonny"));

        let db_path = data_dir.join("memory.db");
        let model_dir = data_dir.join("models");

        Self {
            data_dir,
            db_path,
            model_dir,
            llm: LlmConfig::defaults(),
            embedding: EmbeddingConfig::defaults(),
            clustering: ClusterConfig::defaults(),
            recall: RecallConfig::defaults(),
            confidence: ConfidenceConfig::defaults(),
        }
    }

    pub fn load_test() -> Self {
        let mut s = Self::load();
        s.db_path = PathBuf::from(":memory:");
        s
    }
}

impl LlmConfig {
    fn defaults() -> Self {
        Self {
            api_url: std::env::var("SONNY_LLM_API_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            api_key: std::env::var("SONNY_LLM_API_KEY").unwrap_or_default(),
            model: std::env::var("SONNY_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            max_tokens: 4096,
            timeout_secs: 60,
        }
    }
}

impl EmbeddingConfig {
    fn defaults() -> Self {
        Self {
            api_url: std::env::var("SONNY_EMBEDDING_API_URL").unwrap_or_default(),
            api_key: std::env::var("SONNY_EMBEDDING_API_KEY").unwrap_or_default(),
            model_id: std::env::var("SONNY_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "BAAI/bge-m3".into()),
            dim: std::env::var("SONNY_EMBEDDING_DIM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1024),
            timeout_secs: std::env::var("SONNY_EMBEDDING_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            prefer_vec: true,
        }
    }
}

impl ClusterConfig {
    fn defaults() -> Self {
        Self {
            hac_threshold: 0.25,
            entity_merge_threshold: 0.40,
            incremental_max: 5000,
        }
    }
}

impl RecallConfig {
    fn defaults() -> Self {
        Self {
            semantic_weight: 0.6,
            entity_weight: 0.4,
            top_k: 10,
            top_n: 5,
            max_context_tokens: 1500,
            semantic_threshold: 0.4,
        }
    }
}

impl ConfidenceConfig {
    fn defaults() -> Self {
        Self {
            auto_confirm_vitality: 0.80,
            auto_confirm_sessions: 3,
            auto_demote_vitality: 0.40,
            auto_demote_age_days: 30,
            decay_half_life_days: 90.0,
        }
    }
}

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_home_resolves_existing_directory() {
        let home = dirs_home();
        assert!(
            home.is_absolute(),
            "home dir should be absolute, got {home:?}"
        );
        assert!(
            home.exists(),
            "home dir should exist on this platform, got {home:?}"
        );
    }
}
