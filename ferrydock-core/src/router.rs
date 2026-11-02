use crate::config::Config;
use crate::error::{Error, Result};
use crate::metrics::{Circuit, MetricsStore};
use crate::providers::GenericProvider;
use rand::Rng;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};

pub const OWNER: &str = "ferrydock-v1";

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub native: String,
    pub display: String,
    pub supports_tools: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    pub only_reasoning: bool,
    pub max_input: Option<u32>,
    pub max_output: Option<u32>,
}

pub(crate) struct Inner {
    providers: Vec<Arc<GenericProvider>>,
    pub(crate) catalog: Vec<ModelEntry>,
    native_index: HashMap<String, Vec<String>>,
}

pub struct Engine {
    pub(crate) inner: Arc<RwLock<Inner>>,
    client: Arc<RwLock<reqwest::Client>>,
    sem: Arc<Semaphore>,
    circuits: Arc<std::sync::Mutex<HashMap<String, Circuit>>>,
    metrics: Arc<MetricsStore>,
    pub(crate) config: Arc<RwLock<Config>>,
    pub(crate)     api_key: String,
    #[allow(dead_code)]
    data_dir: PathBuf,
    config_path: PathBuf,
}

impl Engine {
    pub async fn build(data_dir: PathBuf) -> Result<Arc<Engine>> {
        std::fs::create_dir_all(&data_dir)?;
        let config_path = data_dir.join("config.json");
        let config = Config::load(&config_path)?;
        let client = build_client(&config)?;
        let api_key = load_or_create_api_key(&data_dir)?;
        let engine = Engine {
            inner: Arc::new(RwLock::new(Inner {
                providers: vec![],
                catalog: vec![],
                native_index: HashMap::new(),
            })),
            client: Arc::new(RwLock::new(client)),
            sem: Arc::new(Semaphore::new(config.concurrency.max(1))),
            circuits: Arc::new(std::sync::Mutex::new(HashMap::new())),
            metrics: Arc::new(MetricsStore::new()),
            config: Arc::new(RwLock::new(config)),
            api_key,
            data_dir,
            config_path,
        };
        let engine = Arc::new(engine);
        {
            let e = engine.clone();
            e.spawn_background();
        }
        Ok(engine)
    }

    fn spawn_background(self: Arc<Self>) {
        let eng = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                if let Err(e) = eng.reload_and_probe().await {
                    tracing::warn!("periodic reload/probe failed: {e}");
                }
            }
        });
    }

    /// Reload config from disk, rebuild the HTTP client + providers, probe
    /// every provider for models, rebuild the catalog, and re-sync WorkBuddy.
    pub async fn reload_and_probe(&self) -> Result<()> {
        let cfg = Config::load(&self.config_path)?;
        let client = build_client(&cfg)?;
        *self.client.write().await = client;
        *self.config.write().await = cfg.clone();

        let providers: Vec<Arc<GenericProvider>> = cfg
            .providers
            .iter()
            .filter(|p| p.enabled)
            .map(|p| Arc::new(GenericProvider::new(p.clone())))
            .collect();
        for p in &providers {
            self.metrics.ensure(p.id());
        }

        let mut catalog = Vec::new();
        let mut native_index: HashMap<String, Vec<String>> = HashMap::new();
        {
            let client = self.client.read().await.clone();
            for p in &providers {
                match p.list_models(&client).await {
                    Ok(models) => {
                        self.metrics.mark_healthy(p.id(), true);
                        for m in models {
                            let catalog_id = format!("Ferrydock · {} [{}]", m.native, p.id());
                            catalog.push(ModelEntry {
                                id: catalog_id.clone(),
                                name: format!("Ferrydock · {}", m.native),
                                provider_id: p.id().to_string(),
                                native: m.native.clone(),
                                display: m.native.clone(),
                                supports_tools: m.supports_tools,
                                supports_images: m.supports_images,
                                supports_reasoning: m.supports_reasoning,
                                only_reasoning: m.only_reasoning,
                                max_input: m.max_input,
                                max_output: m.max_output,
                            });
                            native_index
                                .entry(m.native.clone())
                                .or_default()
                                .push(p.id().to_string());
                        }
                    }
                    Err(e) => {
                        self.metrics.mark_healthy(p.id(), false);
                        tracing::warn!("provider {} probe failed: {}", p.id(), e);
                    }
                }
            }
        }

        *self.inner.write().await = Inner {
            providers,
            catalog,
            native_index,
        };
        let _ = self.sync_now().await;
        Ok(())
    }

    /// Route a chat completion request across candidate providers with
    /// failover, circuit breaking, bounded concurrency and exponential backoff.
    pub async fn chat(&self, body: Value) -> Result<reqwest::Response> {
        let catalog_id = body
            .get("model")
            .and_then(|m| m.as_str())
            .ok_or_else(|| Error::NoModel("missing model field".into()))?
            .to_string();

        let (entry, cands) = {
            let inner = self.inner.read().await;
            let entry = inner
                .catalog
                .iter()
                .find(|e| e.id == catalog_id)
                .cloned()
                .ok_or_else(|| Error::NoModel(catalog_id.clone()))?;
            let mut cands: Vec<Arc<GenericProvider>> = Vec::new();
            if let Some(ids) = inner.native_index.get(&entry.native) {
                for pid in ids {
                    if let Some(p) = inner.providers.iter().find(|p| p.id() == pid) {
                        cands.push(p.clone());
                    }
                }
            }
            cands.sort_by_key(|p| {
                let home = if p.id() == entry.provider_id { 0u32 } else { 1u32 };
                home + p.cfg.priority
            });
            (entry, cands)
        };

        if cands.is_empty() {
            return Err(Error::AllFailed(format!(
                "no provider for model {}",
                entry.native
            )));
        }

        let cfg = self.config.read().await.clone();
        let mut last_err: Option<Error> = None;

        for provider in &cands {
            if self.circuit_open(provider.id()) {
                last_err = Some(Error::CircuitOpen(provider.id().into()));
                continue;
            }
            for attempt in 0..=cfg.max_retries {
                let _permit = self
                    .sem
                    .acquire()
                    .await
                    .map_err(|_| Error::AllFailed("semaphore closed".into()))?;
                let start = Instant::now();
                let client = self.client.read().await.clone();
                match provider.chat(&entry.native, &body, &client).await {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            let ms = start.elapsed().as_millis() as f64;
                            self.metrics.record_success(provider.id(), ms);
                            self.circuit_success(provider.id());
                            return Ok(resp);
                        }
                        let code = status.as_u16();
                        let txt = resp.text().await.unwrap_or_default();
                        self.metrics
                            .record_error(provider.id(), &format!("upstream {code}: {txt}"));
                        self.circuit_failure(provider.id());
                        last_err = Some(Error::Upstream {
                            status: code,
                            body: txt.clone(),
                        });
                        // Fatal client error (4xx except 429): do not retry this provider.
                        if code != 429 && (400..500).contains(&code) {
                            break;
                        }
                    }
                    Err(e) => {
                        self.metrics.record_error(provider.id(), &e.to_string());
                        self.circuit_failure(provider.id());
                        last_err = Some(e);
                    }
                }
                if attempt < cfg.max_retries {
                    tokio::time::sleep(backoff(cfg.retry_base_delay_ms, attempt)).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| Error::AllFailed("all providers exhausted".into())))
    }

    pub async fn addr(&self) -> String {
        let c = self.config.read().await;
        format!("{}:{}", c.bind, c.port)
    }

    /// Local gateway base URL (used by the frontend / for display).
    pub async fn gateway_url(&self) -> String {
        format!("http://{}:{}", self.config.read().await.bind, self.config.read().await.port)
    }

    /// On-disk data directory (config.json + api-key live here).
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    /// The generated API key clients must present (WorkBuddy + the desktop UI).
    pub fn api_key(&self) -> String {
        self.api_key.clone()
    }

    /// Merge a JSON patch into the config file on disk, then reload + re-probe.
    /// Shared by the HTTP `/admin/config` endpoint and the desktop UI.
    pub async fn update_config(&self, patch: Value) -> Result<()> {
        let path = self.config_path.clone();
        let mut cur = {
            let cfg = self.config.read().await;
            serde_json::to_value(&*cfg).unwrap_or(Value::Null)
        };
        if let (Some(obj), Some(patch_obj)) = (cur.as_object_mut(), patch.as_object()) {
            for (k, v) in patch_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
        let txt = serde_json::to_string_pretty(&cur)
            .map_err(|e| Error::Config(format!("serialize config: {e}")))?;
        std::fs::write(&path, txt).map_err(|e| Error::Config(format!("write config: {e}")))?;
        self.reload_and_probe().await
    }

    pub async fn config_path(&self) -> PathBuf {
        self.config_path.clone()
    }

    pub async fn health(&self) -> Value {
        let inner = self.inner.read().await;
        json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "providers": inner.providers.iter().map(|p| p.id()).collect::<Vec<_>>(),
            "models": inner.catalog.len(),
        })
    }

    pub async fn status(&self) -> Value {
        let inner = self.inner.read().await;
        json!({
            "providers": self.metrics.snapshot(),
            "models": inner.catalog,
            "config": self.redacted_config().await,
        })
    }

    pub async fn sync_now(&self) -> Result<()> {
        let cfg = self.config.read().await.clone();
        let inner = self.inner.read().await;
        let url = format!("http://{}:{}/v1/chat/completions", cfg.bind, cfg.port);
        let mut entries = Vec::new();
        for e in &inner.catalog {
            if !self.provider_healthy(&e.provider_id) {
                continue;
            }
            entries.push(json!({
                "id": e.id,
                "name": e.name,
                "vendor": "Custom",
                "url": url,
                "apiKey": self.api_key,
                "supportsToolCall": e.supports_tools,
                "supportsImages": e.supports_images,
                "supportsReasoning": e.supports_reasoning,
                "onlyReasoning": e.only_reasoning,
                "reasoning": {"supportedEfforts": [], "canDisableThinking": false},
                "ocFerryOwner": OWNER,
                "maxInputTokens": e.max_input.unwrap_or(200000),
                "maxOutputTokens": e.max_output.unwrap_or(32000),
            }));
        }
        drop(inner);
        // sync_workbuddy does blocking file I/O; run it off the async worker.
        let path_owned = cfg.workbuddy_path();
        tokio::task::spawn_blocking(move || crate::sync::sync_workbuddy(&path_owned, &entries))
            .await
            .map_err(|e| Error::Config(format!("sync task: {e}")))?
    }

    /// Remove our entries from WorkBuddy (used on exit). Respects
    /// `remove_on_exit` — when false we leave the registered models in place.
    pub async fn cleanup(&self) -> Result<()> {
        let cfg = self.config.read().await.clone();
        if !cfg.remove_on_exit {
            tracing::info!("remove_on_exit=false; leaving WorkBuddy entries in place");
            return Ok(());
        }
        let path_owned = cfg.workbuddy_path();
        tokio::task::spawn_blocking(move || crate::sync::sync_workbuddy(&path_owned, &[]))
            .await
            .map_err(|e| Error::Config(format!("cleanup task: {e}")))?
    }

    fn provider_healthy(&self, id: &str) -> bool {
        self.metrics
            .snapshot()
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.healthy)
            .unwrap_or(false)
    }

    pub async fn redacted_config(&self) -> Value {
        let cfg = self.config.read().await;
        let mut v = serde_json::to_value(&*cfg).unwrap_or(Value::Null);
        if let Some(arr) = v.get_mut("providers").and_then(|p| p.as_array_mut()) {
            for p in arr {
                if let Some(ak) = p.get_mut("api_key") {
                    if !ak.is_null()
                        && ak.as_str().map(|s| !s.is_empty()).unwrap_or(false)
                    {
                        *ak = json!("***");
                    }
                }
            }
        }
        v
    }

    // ---- circuit breaker helpers ----
    fn circuit_open(&self, id: &str) -> bool {
        let m = self.circuits.lock().unwrap();
        m.get(id).map(|c| c.is_open()).unwrap_or(false)
    }
    fn circuit_failure(&self, id: &str) {
        let mut m = self.circuits.lock().unwrap();
        let c = m.entry(id.to_string()).or_insert_with(Circuit::new);
        c.register_failure();
        let open = c.is_open();
        drop(m);
        self.metrics.set_circuit(id, open);
    }
    fn circuit_success(&self, id: &str) {
        let mut m = self.circuits.lock().unwrap();
        let c = m.entry(id.to_string()).or_insert_with(Circuit::new);
        c.register_success();
        drop(m);
        self.metrics.set_circuit(id, false);
    }
}

fn backoff(base_ms: u64, attempt: u32) -> Duration {
    let exp = base_ms.saturating_mul(1u64 << attempt.min(6));
    let jitter = rand::random::<u64>() % 200;
    Duration::from_millis(exp + jitter)
}

fn build_client(cfg: &Config) -> Result<reqwest::Client> {
    // Start with no proxy so the ambient HTTP(S)_PROXY env never applies
    // implicitly; we only add a proxy when the user explicitly configures one.
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(cfg.request_timeout_secs));
    if let Some(proxy) = resolve_proxy(cfg)? {
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| Error::Client(format!("build client: {e}")))
}

fn resolve_proxy(cfg: &Config) -> Result<Option<reqwest::Proxy>> {
    if let Some(url) = &cfg.proxy_url {
        if !url.is_empty() {
            return Ok(Some(
                reqwest::Proxy::all(url).map_err(|e| Error::Config(format!("proxy url: {e}")))?,
            ));
        }
    }
    if cfg.use_system_proxy {
        if let Some(url) = scutil_proxy() {
            return Ok(Some(
                reqwest::Proxy::all(&url).map_err(|e| Error::Config(format!("proxy: {e}")))?,
            ));
        }
    }
    Ok(None)
}

#[cfg(target_os = "macos")]
fn scutil_proxy() -> Option<String> {
    let out = std::process::Command::new("scutil").arg("--proxy").output().ok()?;
    let txt = String::from_utf8_lossy(&out.stdout);
    let mut host = None;
    let mut port = None;
    let mut enabled = false;
    for line in txt.lines() {
        let line = line.trim();
        if (line.starts_with("HTTPEnable") || line.starts_with("HTTPSEnable"))
            && line.chars().last() == Some('1')
        {
            enabled = true;
        }
        if let Some(v) = line.strip_prefix("HTTPProxy:") {
            host = Some(v.trim().to_string());
        }
        if let Some(v) = line.strip_prefix("HTTPSProxy:") {
            if host.is_none() {
                host = Some(v.trim().to_string());
            }
        }
        if let Some(v) = line.strip_prefix("HTTPPort:") {
            port = v.trim().parse::<u16>().ok();
        }
        if let Some(v) = line.strip_prefix("HTTPSPort:") {
            if port.is_none() {
                port = v.trim().parse::<u16>().ok();
            }
        }
    }
    if enabled {
        let h = host?;
        let p = port.unwrap_or(8080);
        return Some(format!("http://{h}:{p}"));
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn scutil_proxy() -> Option<String> {
    None
}

fn load_or_create_api_key(dir: &Path) -> Result<String> {
    let p = dir.join("api-key");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.r#gen();
    let key = bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    std::fs::write(&p, &key)?;
    Ok(key)
}
