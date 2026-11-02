pub mod opencode_local;
pub use opencode_local::OpenCodeLocalProvider;

use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{Error, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub native: String,
    pub supports_tools: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    pub only_reasoning: bool,
    pub max_input: Option<u32>,
    pub max_output: Option<u32>,
}

pub fn default_base_url(kind: ProviderKind) -> &'static str {
    match kind {
        // OpenCode Zen: the OpenAI-compatible endpoint reachable by EXTERNAL
        // clients. OpenCode's free `/inference` tier is 403 to anything but the
        // in-client OpenCode runtime (that's why OC Ferry shells out to a local
        // `opencode serve`), so a standalone gateway must use Zen with a key.
        ProviderKind::OpenCodeFree => "https://opencode.ai/zen/v1",
        ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
        ProviderKind::Ollama => "http://localhost:11434/v1",
        ProviderKind::Custom => "http://localhost:11434/v1",
        // OpenCodeLocal's base URL is injected at runtime from the local
        // `opencode serve`; this default is never used for it.
        ProviderKind::OpenCodeLocal => "http://127.0.0.1:0/opencode-local",
    }
}

fn info_from_id(id: &str) -> ModelInfo {
    let lower = id.to_lowercase();
    let supports_images = lower.contains("vl")
        || lower.contains("vision")
        || lower.contains("image")
        || lower.contains("multimodal");
    ModelInfo {
        native: id.to_string(),
        supports_tools: true,
        supports_images,
        supports_reasoning: true,
        only_reasoning: false,
        max_input: None,
        max_output: None,
    }
}

/// A provider is a single upstream that speaks the OpenAI-compatible
/// `/v1/chat/completions` protocol. Ollama is handled specially for listing.
pub struct GenericProvider {
    pub cfg: ProviderConfig,
    pub kind: ProviderKind,
    pub base_url: String,
}

impl GenericProvider {
    pub fn new(cfg: ProviderConfig) -> Self {
        let kind = cfg.kind;
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| default_base_url(kind).to_string());
        let base = base.trim_end_matches('/').to_string();
        Self {
            cfg,
            kind,
            base_url: base,
        }
    }

    pub fn id(&self) -> &str {
        &self.cfg.id
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn ollama_root(&self) -> String {
        if self.kind == ProviderKind::Ollama {
            self.base_url.trim_end_matches('/').trim_end_matches("/v1").to_string()
        } else {
            self.base_url.clone()
        }
    }

    fn auth<'a>(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.cfg.api_key {
            Some(k) if !k.is_empty() => req.bearer_auth(k),
            _ => req,
        }
    }

    pub async fn list_models(&self, client: &Client) -> Result<Vec<ModelInfo>> {
        let url = if self.kind == ProviderKind::Ollama {
            format!("{}/api/tags", self.ollama_root())
        } else {
            format!("{}/models", self.base_url)
        };
        let req = self.auth(client.get(&url));
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Client(format!("list_models {url}: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Upstream { status, body });
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| Error::Client(format!("parse models: {e}")))?;
        let mut out = Vec::new();
        if self.kind == ProviderKind::Ollama {
            if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                for m in arr {
                    if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                        out.push(info_from_id(name));
                    }
                }
            }
        } else if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                    out.push(info_from_id(id));
                }
            }
        }
        // Optional include filter.
        if let Some(filt) = &self.cfg.model_filter {
            if !filt.is_empty() {
                out.retain(|i| {
                    filt.iter()
                        .any(|f| i.native == *f || i.native.contains(f))
                });
            }
        }
        // Optional forced models.
        if let Some(extra) = &self.cfg.extra_models {
            for e in extra {
                if !out.iter().any(|i| i.native == *e) {
                    out.push(info_from_id(e));
                }
            }
        }
        Ok(out)
    }

    /// Send a chat completion. The body's `model` field is rewritten to the
    /// provider-native model id before sending. Returns the raw upstream
    /// response so the router can decide on streaming vs JSON.
    pub async fn chat(
        &self,
        native_model: &str,
        body: &Value,
        client: &Client,
    ) -> Result<reqwest::Response> {
        let mut b = body.clone();
        if let Some(obj) = b.as_object_mut() {
            obj.insert("model".to_string(), json!(native_model));
        }
        let req = self.auth(client.post(self.chat_url()).json(&b));
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Client(format!("chat {}: {e}", self.chat_url())))?;
        Ok(resp)
    }
}

/// Outcome of a provider chat: either a live upstream response to forward, or
/// a finished OpenAI-shaped JSON body produced by a native (non-OpenAI) provider.
pub enum ProviderChatResult {
    Upstream(reqwest::Response),
    Json(String),
}

/// A provider is either an OpenAI-compatible upstream (`GenericProvider`) or a
/// native-protocol upstream (`OpenCodeLocalProvider`). The router treats them
/// uniformly for model listing and routing.
pub enum AnyProvider {
    OpenAI(GenericProvider),
    Native(OpenCodeLocalProvider),
}

impl AnyProvider {
    pub fn id(&self) -> &str {
        match self {
            AnyProvider::OpenAI(p) => p.id(),
            AnyProvider::Native(p) => p.id(),
        }
    }

    pub fn priority(&self) -> u32 {
        match self {
            AnyProvider::OpenAI(p) => p.cfg.priority,
            AnyProvider::Native(p) => p.cfg.priority,
        }
    }

    pub async fn list_models(&self, client: &Client) -> Result<Vec<ModelInfo>> {
        match self {
            AnyProvider::OpenAI(p) => p.list_models(client).await,
            AnyProvider::Native(p) => p.list_models(client).await,
        }
    }

    pub async fn chat(
        &self,
        native: &str,
        body: &Value,
        client: &Client,
    ) -> Result<ProviderChatResult> {
        match self {
            AnyProvider::OpenAI(p) => {
                Ok(ProviderChatResult::Upstream(p.chat(native, body, client).await?))
            }
            AnyProvider::Native(p) => {
                Ok(ProviderChatResult::Json(p.chat(native, body, client).await?))
            }
        }
    }
}
