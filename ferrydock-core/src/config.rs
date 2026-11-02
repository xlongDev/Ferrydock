use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenCode Zen: OpenAI-compatible endpoint reachable by external clients,
    /// requires an API key. Disabled by default (needs a key).
    OpenCodeFree,
    /// Local OpenCode runtime: Ferrydock downloads/launches an isolated
    /// `opencode serve`, then talks to it over OpenCode's native protocol.
    /// No API key needed — this is the keyless free-model path (same as OC Ferry).
    OpenCodeLocal,
    OpenRouter,
    Ollama,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Override base URL (required for Custom; optional for others).
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub priority: u32,
    /// If set, only keep models whose id equals or contains one of these.
    pub model_filter: Option<Vec<String>>,
    /// Force-add these model ids even if the upstream does not list them.
    pub extra_models: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    #[serde(default = "default_base_delay")]
    pub retry_base_delay_ms: u64,
    pub remove_on_exit: bool,
    pub use_system_proxy: bool,
    pub proxy_url: Option<String>,
    pub workbuddy_models_path: Option<PathBuf>,
    pub providers: Vec<ProviderConfig>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: "127.0.0.1".into(),
            port: 41980,
            concurrency: 8,
            request_timeout_secs: 120,
            max_retries: 3,
            retry_base_delay_ms: 500,
            remove_on_exit: true,
            use_system_proxy: false,
            proxy_url: None,
            workbuddy_models_path: None,
            providers: default_providers(),
            log_level: "info".into(),
        }
    }
}

fn default_bind() -> String {
    "127.0.0.1".into()
}
fn default_port() -> u16 {
    41980
}
fn default_concurrency() -> usize {
    8
}
fn default_timeout() -> u64 {
    120
}
fn default_retries() -> u32 {
    3
}
fn default_base_delay() -> u64 {
    500
}
fn default_log_level() -> String {
    "info".into()
}

fn default_providers() -> Vec<ProviderConfig> {
    vec![
        // Keyless free models via a locally-managed OpenCode runtime (OC Ferry's path).
        ProviderConfig {
            id: "opencode-local".into(),
            kind: ProviderKind::OpenCodeLocal,
            enabled: true,
            base_url: None,
            api_key: None,
            priority: 0,
            model_filter: None,
            extra_models: None,
        },
        // OpenCode Zen (external, needs a key) — off until the user supplies one.
        ProviderConfig {
            id: "opencode-free".into(),
            kind: ProviderKind::OpenCodeFree,
            enabled: false,
            base_url: None,
            api_key: None,
            priority: 1,
            model_filter: None,
            extra_models: None,
        },
        ProviderConfig {
            id: "openrouter".into(),
            kind: ProviderKind::OpenRouter,
            enabled: true,
            base_url: None,
            api_key: None,
            priority: 2,
            model_filter: None,
            extra_models: None,
        },
        ProviderConfig {
            id: "ollama".into(),
            kind: ProviderKind::Ollama,
            enabled: false,
            base_url: None,
            api_key: None,
            priority: 3,
            model_filter: None,
            extra_models: None,
        },
    ]
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Config> {
        if !path.exists() {
            let def = Config::default();
            def.save(path)?;
            return Ok(def);
        }
        let txt = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
        let cfg: Config =
            serde_json::from_str(&txt).map_err(|e| Error::Config(format!("parse: {e}")))?;
        Ok(cfg)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let txt = serde_json::to_string_pretty(self)?;
        std::fs::write(path, txt)?;
        Ok(())
    }

    /// Resolve the WorkBuddy models.json path (~/.workbuddy/models.json).
    pub fn workbuddy_path(&self) -> PathBuf {
        self.workbuddy_models_path
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".workbuddy/models.json"))
    }
}
