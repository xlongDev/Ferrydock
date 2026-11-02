//! Local OpenCode runtime manager.
//!
//! Mirrors OC Ferry's approach: instead of hitting OpenCode's free `/inference`
//! tier directly (which rejects external clients with 403), we download and
//! launch an *isolated* `opencode serve` locally, then talk to it over
//! OpenCode's native protocol. The local runtime carries OpenCode's internal
//! client identity, so it can reach the free models without any API key.

use crate::error::{Error, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command as TokioCommand;

const PKG_NAME: &str = "opencode-ai";
const BINARY: &str = "opencode";
const REGISTRY: &str = "https://registry.npmjs.org";

pub struct OpenCodeRuntime {
    #[allow(dead_code)]
    binary: PathBuf,
    pub port: u16,
    password: String,
    pub base_url: String,
    child: Option<tokio::process::Child>,
}

impl OpenCodeRuntime {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Locate or download the opencode binary, launch an isolated
    /// `opencode serve`, and wait until `/global/health` reports healthy.
    ///
    /// `proxy_url`, if set, is only used when downloading the binary from npm;
    /// all loopback communication with the runtime is always no-proxy.
    pub async fn ensure(data_dir: &Path, proxy_url: Option<&str>) -> Result<OpenCodeRuntime> {
        // The local runtime is reached over loopback, so never proxy it.
        let local_http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| Error::Config(format!("build local client: {e}")))?;
        // Downloading the binary may need the system proxy.
        let mut dlb = reqwest::Client::builder().timeout(Duration::from_secs(180));
        if let Some(p) = proxy_url {
            if !p.is_empty() {
                dlb = dlb.proxy(
                    reqwest::Proxy::all(p).map_err(|e| Error::Config(format!("proxy: {e}")))?,
                );
            } else {
                dlb = dlb.no_proxy();
            }
        } else {
            dlb = dlb.no_proxy();
        }
        let download_http = dlb
            .build()
            .map_err(|e| Error::Config(format!("build download client: {e}")))?;
        let binary = resolve_binary(data_dir, &download_http).await?;
        let port = free_port().map_err(|e| Error::Config(format!("find free port: {e}")))?;
        let password = random_hex(24);
        let base_url = format!("http://127.0.0.1:{port}");
        let isolated = data_dir.join("opencode").join("isolated");
        for d in ["config", "data", "cache", "state", "project"] {
            std::fs::create_dir_all(isolated.join(d))
                .map_err(|e| Error::Config(format!("mkdir {d}: {e}")))?;
        }
        let config_content = isolated_config();

        let mut cmd = TokioCommand::new(&binary);
        cmd.args(["serve", "--pure", "--hostname", "127.0.0.1", "--port", &port.to_string()]);
        cmd.current_dir(isolated.join("project"));
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        // Whitelist env so we don't leak other providers' keys into the runtime.
        for k in [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "LANG",
            "TMPDIR",
            "TEMP",
            "TMP",
            "SHELL",
            "SSL_CERT_FILE",
            "NODE_EXTRA_CA_CERTS",
        ] {
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
        // Isolated XDG dirs keep the runtime from touching the user's real config.
        cmd.env("XDG_CONFIG_HOME", isolated.join("config"));
        cmd.env("XDG_DATA_HOME", isolated.join("data"));
        cmd.env("XDG_CACHE_HOME", isolated.join("cache"));
        cmd.env("XDG_STATE_HOME", isolated.join("state"));
        cmd.env("OPENCODE_SERVER_PASSWORD", &password);
        cmd.env("OPENCODE_SERVER_USERNAME", "opencode");
        cmd.env("OPENCODE_DISABLE_AUTOUPDATE", "true");
        cmd.env("OPENCODE_DISABLE_PROJECT_CONFIG", "true");
        cmd.env("OPENCODE_DISABLE_CLAUDE_CODE", "true");
        cmd.env("OPENCODE_DISABLE_EXTERNAL_SKILLS", "true");
        cmd.env("OPENCODE_CONFIG_CONTENT", config_content);

        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Config(format!("spawn opencode: {e}")))?;

        // Readiness: poll /global/health (not just "process alive").
        let mut healthy = false;
        for _ in 0..120 {
            if let Some(code) = child
                .try_wait()
                .map_err(|e| Error::Config(format!("child wait: {e}")))?
            {
                return Err(Error::Config(format!(
                    "opencode exited before becoming healthy (code={code:?})"
                )));
            }
            match local_http
                .get(format!("{base_url}/global/health"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(r) => {
                    if let Ok(v) = r.json::<Value>().await {
                        if v.get("healthy").and_then(|x| x.as_bool()).unwrap_or(false) {
                            healthy = true;
                            break;
                        }
                    }
                }
                Err(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if !healthy {
            let _ = child.start_kill();
            return Err(Error::Config(
                "opencode serve did not become healthy in time".into(),
            ));
        }
        tracing::info!("local OpenCode runtime up at {base_url}");
        Ok(OpenCodeRuntime {
            binary,
            port,
            password,
            base_url,
            child: Some(child),
        })
    }

    pub async fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(4), c.wait()).await;
        }
    }
}

impl Drop for OpenCodeRuntime {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.start_kill();
        }
    }
}

/// Find an existing opencode binary, or download one from npm.
async fn resolve_binary(data_dir: &Path, http: &Client) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("FERRY_OPENCODE_PATH") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    let candidates = [
        home.join(".opencode/bin").join(BINARY),
        PathBuf::from("/opt/homebrew/bin").join(BINARY),
        PathBuf::from("/usr/local/bin").join(BINARY),
    ];
    for c in &candidates {
        if is_executable(c) {
            return Ok(c.clone());
        }
    }
    download_binary(data_dir, http).await
}

async fn download_binary(data_dir: &Path, http: &Client) -> Result<PathBuf> {
    let meta_url = format!("{REGISTRY}/{PKG_NAME}/latest");
    let resp = http
        .get(&meta_url)
        .send()
        .await
        .map_err(|e| Error::Client(format!("npm metadata: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Upstream {
            status: resp.status().as_u16(),
            body: "npm metadata".into(),
        });
    }
    let meta: Value = resp
        .json()
        .await
        .map_err(|e| Error::Client(format!("npm json: {e}")))?;
    let version = meta
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("npm: no version".into()))?;
    let dist = meta
        .get("dist")
        .ok_or_else(|| Error::Config("npm: no dist".into()))?;
    let tarball = dist
        .get("tarball")
        .and_then(|t| t.as_str())
        .ok_or_else(|| Error::Config("npm: no tarball".into()))?;
    let integrity = dist.get("integrity").and_then(|i| i.as_str()).unwrap_or("");

    let target = data_dir.join("runtime").join(version).join(BINARY);
    if target.exists() {
        if let Ok(out) = TokioCommand::new(&target).arg("--version").output().await {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if s.trim() == version {
                    return Ok(target);
                }
            }
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Config(format!("mkdir runtime: {e}")))?;
    }

    let bytes = http
        .get(tarball)
        .send()
        .await
        .map_err(|e| Error::Client(format!("download tarball: {e}")))?
        .bytes()
        .await
        .map_err(|e| Error::Client(format!("read tarball: {e}")))?;

    if integrity.starts_with("sha512-") {
        let expected = &integrity["sha512-".len()..];
        use sha2::Digest;
        let mut h = sha2::Sha512::new();
        h.update(&bytes);
        let got = base64::engine::general_purpose::STANDARD.encode(h.finalize());
        if got != expected {
            return Err(Error::Config(
                "opencode download integrity mismatch (tarball may be tampered)".into(),
            ));
        }
    }

    extract_binary(&bytes, &target)
        .map_err(|e| Error::Config(format!("extract opencode: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
    }
    tracing::info!("downloaded opencode {version} -> {}", target.display());
    Ok(target)
}

fn extract_binary(bytes: &[u8], target: &Path) -> std::io::Result<()> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let member = std::path::Path::new("package/bin").join(BINARY);
    for entry in archive.entries()? {
        let mut e = entry?;
        if e.path()?.as_os_str() == member.as_os_str() {
            e.unpack(target)?;
            return Ok(());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "opencode binary not found in tarball",
    ))
}

fn free_port() -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = l.local_addr()?.port();
    drop(l);
    Ok(port)
}

fn is_executable(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn random_hex(n: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..n).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

/// Minimal isolated config: a single plain-text chat agent with all native
/// tools denied, so the runtime can never act on the local machine.
fn isolated_config() -> String {
    serde_json::json!({
        "permission": { "*": "deny" },
        "autoupdate": false,
        "share": "disabled",
        "agent": {
            "oc-ferry-chat": {
                "mode": "primary",
                "description": "Plain-text external chat (Ferrydock bridge)",
                "prompt": "Reply in plain text only. Never invoke any tools or perform local actions.",
                "permission": { "*": "deny" }
            }
        }
    })
    .to_string()
}
