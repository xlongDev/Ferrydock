//! OpenCode native-protocol provider.
//!
//! Talks to a locally-managed `opencode serve` (see `runtime.rs`) over
//! OpenCode's native HTTP API (not OpenAI-compatible). Auth is
//! `Basic opencode:<password>`. This is how we reach OpenCode's free models
//! without any API key — exactly OC Ferry's trick, minus the tool/envelope
//! machinery (we only do plain-text chat here).

use crate::config::ProviderConfig;
use crate::error::{Error, Result};
use crate::providers::{info_from_id, ModelInfo};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct OpenCodeLocalProvider {
    pub cfg: ProviderConfig,
    pub base_url: String,
    pub password: String,
    /// Loopback-only client (always no-proxy) for talking to the runtime.
    client: reqwest::Client,
}

impl OpenCodeLocalProvider {
    pub fn new(cfg: ProviderConfig, base_url: String, password: String) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("build opencode-local client");
        Self {
            cfg,
            base_url,
            password,
            client,
        }
    }

    pub fn id(&self) -> &str {
        &self.cfg.id
    }

    pub async fn list_models(&self, _client: &Client) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/provider", self.base_url);
        let resp = self
            .client
            .get(&url)
            .basic_auth("opencode", Some(&self.password))
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
            .map_err(|e| Error::Client(format!("parse provider: {e}")))?;
        let mut out = Vec::new();
        if let Some(all) = v.get("all").and_then(|a| a.as_array()) {
            if let Some(oc) = all
                .iter()
                .find(|p| p.get("id").and_then(|i| i.as_str()) == Some("opencode"))
            {
                if let Some(models) = oc.get("models").and_then(|m| m.as_object()) {
                    for key in models.keys() {
                        let mut info = info_from_id(key);
                        // Free inference through the local runtime has no tools.
                        info.supports_tools = false;
                        out.push(info);
                    }
                }
            }
        }
        if let Some(filt) = &self.cfg.model_filter {
            if !filt.is_empty() {
                out.retain(|i| filt.iter().any(|f| i.native == *f || i.native.contains(f)));
            }
        }
        if let Some(extra) = &self.cfg.extra_models {
            for e in extra {
                if !out.iter().any(|i| i.native == *e) {
                    out.push(info_from_id(e));
                }
            }
        }
        Ok(out)
    }

    /// Send a plain-text chat. Returns an OpenAI-shaped completion JSON string
    /// (the router wraps this into the response).
    pub async fn chat(
        &self,
        native_model: &str,
        body: &Value,
        _client: &Client,
    ) -> Result<String> {
        // 1. open a session
        let session_url = format!("{}/session", self.base_url);
        let sresp = self
            .client
            .post(&session_url)
            .basic_auth("opencode", Some(&self.password))
            .json(&json!({"title": "Ferrydock", "permission": []}))
            .send()
            .await
            .map_err(|e| Error::Client(format!("session {session_url}: {e}")))?;
        if !sresp.status().is_success() {
            let status = sresp.status().as_u16();
            let b = sresp.text().await.unwrap_or_default();
            return Err(Error::Upstream { status, body: b });
        }
        let sid = sresp
            .json::<Value>()
            .await
            .map_err(|e| Error::Client(format!("parse session: {e}")))?
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| Error::Client("session response missing id".into()))?
            .to_string();

        // 2. last user message text
        let text = last_user_text(body);

        // 3. post the message (oc-ferry-chat: plain text, no tools)
        let msg_url = format!("{}/session/{sid}/message", self.base_url);
        let payload = json!({
            "model": {"providerID": "opencode", "modelID": native_model},
            "agent": "oc-ferry-chat",
            "parts": [{"type": "text", "text": text}],
        });
        let mresp = self
            .client
            .post(&msg_url)
            .basic_auth("opencode", Some(&self.password))
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::Client(format!("message {msg_url}: {e}")))?;
        if !mresp.status().is_success() {
            let status = mresp.status().as_u16();
            let b = mresp.text().await.unwrap_or_default();
            return Err(Error::Upstream { status, body: b });
        }
        let v: Value = mresp
            .json()
            .await
            .map_err(|e| Error::Client(format!("parse message: {e}")))?;

        // 4. extract text parts
        let mut out = String::new();
        if let Some(parts) = v.get("parts").and_then(|p| p.as_array()) {
            for p in parts {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                        out.push_str(t);
                    }
                }
            }
        }
        if let Some(err) = v.get("info").and_then(|i| i.get("error")) {
            if err.get("message").is_some() {
                let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("openCode error");
                return Err(Error::Upstream {
                    status: 502,
                    body: msg.to_string(),
                });
            }
        }

        Ok(openai_wrap(native_model, &out))
    }
}

fn last_user_text(body: &Value) -> String {
    let mut text = String::new();
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for m in msgs.iter().rev() {
            if m.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(c) = m.get("content").and_then(|c| c.as_str()) {
                    text = c.to_string();
                } else if let Some(arr) = m.get("content").and_then(|c| c.as_array()) {
                    for part in arr {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
                break;
            }
        }
    }
    text
}

fn openai_wrap(model: &str, content: &str) -> String {
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let id = format!("chatcmpl-fd-{created}");
    json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    })
    .to_string()
}
