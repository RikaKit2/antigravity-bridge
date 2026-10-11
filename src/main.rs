mod adapter;
mod auth;
mod config;
mod server;
mod types;

use auth::TokenManager;
use clap::Parser;
use config::AppConfig;
use server::{create_router, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "antigravity-bridge")]
#[command(
    about = "Universal OpenAI-to-Antigravity Chat & FIM completions bridge with automatic fallback"
)]
struct Cli {
    #[arg(short, long, env = "CONFIG_PATH")]
    config: Option<PathBuf>,

    #[arg(short, long, env = "BRIDGE_PORT")]
    port: Option<u16>,

    #[arg(long, env = "BRIDGE_HOST")]
    host: Option<String>,

    #[arg(long, env = "PRIMARY_MODEL")]
    primary: Option<String>,

    #[arg(long, env = "UPSTREAM_URL")]
    upstream: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "antigravity_bridge=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    // Check config locations
    let mut config = if let Some(path) = &cli.config {
        AppConfig::load_or_default(Some(path))
    } else {
        let home_config = std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(format!("{}/.config/antigravity-bridge/config.toml", h)));
        if let Some(hp) = &home_config {
            if hp.exists() {
                AppConfig::load_or_default(Some(hp))
            } else {
                AppConfig::load_or_default(None)
            }
        } else {
            AppConfig::load_or_default(None)
        }
    };

    // Override with CLI flags if provided
    if let Some(port) = cli.port {
        config.server.port = port;
    }
    if let Some(host) = cli.host {
        config.server.host = host;
    }
    if let Some(primary) = cli.primary {
        config.routing.primary = primary;
    }
    if let Some(upstream) = cli.upstream {
        config.server.upstream_url = upstream;
    }

    info!(
        "Primary model: {}, Fallbacks: {:?}",
        config.routing.primary, config.routing.fallbacks
    );
    info!("Upstream URL: {}", config.server.upstream_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .build()?;

    let token_manager = TokenManager::new();

    let host = config.server.host.clone();
    let port = config.server.port;
    let config_arc = Arc::new(RwLock::new(config));

    let state = AppState {
        client,
        token_manager,
        config: config_arc,
    };

    let app = create_router(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("Antigravity Bridge listening on http://{}", addr);
    info!("OpenAI Chat endpoint: http://{}/v1/chat/completions", addr);
    info!("OpenAI Models endpoint: http://{}/v1/models", addr);
    info!("Zed FIM endpoint: http://{}/v1/completions", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Antigravity Bridge shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
