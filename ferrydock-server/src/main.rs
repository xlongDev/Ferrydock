use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Strip ambient proxy env vars (Clash / system proxy injects HTTP_PROXY).
    // Ferrydock must reach localhost upstreams (Ollama, local gateways) directly;
    // an explicit proxy is only ever used when the user sets proxy_url or
    // use_system_proxy in config. reqwest reads these at client-build time, so
    // removing them here guarantees it never inherits an implicit proxy.
    for v in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        std::env::remove_var(v);
    }

    let data_dir = std::env::args()
        .find_map(|a| a.strip_prefix("--data-dir=").map(PathBuf::from))
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Ferrydock")
        });
    std::fs::create_dir_all(&data_dir)?;
    tracing::info!("Ferrydock data dir: {}", data_dir.display());

    let engine = ferrydock_core::Engine::build(data_dir).await?;
    tracing::info!("Ferrydock engine ready");

    // On shutdown (Ctrl-C / SIGTERM), remove our entries from WorkBuddy first.
    let e2 = engine.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down; cleaning WorkBuddy entries");
        if let Err(e) = e2.cleanup().await {
            tracing::warn!("cleanup failed: {e}");
        }
        std::process::exit(0);
    });

    ferrydock_core::server::serve(engine.clone()).await?;
    Ok(())
}

/// Resolves when the process receives a termination signal: SIGINT (Ctrl-C) on
/// all platforms, plus SIGTERM on Unix (what `pkill`/process managers send).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = sigint.recv() => { tracing::info!("received SIGINT"); }
            _ = sigterm.recv() => { tracing::info!("received SIGTERM"); }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
