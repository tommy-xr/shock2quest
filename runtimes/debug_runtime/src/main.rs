// Debug Runtime - HTTP-controlled game runtime for LLM testing and automation
//
// This runtime provides a localhost-only HTTP API for controlling the game,
// enabling LLMs and automation scripts to test gameplay, debug issues, and
// validate changes without requiring human interaction.

use axum::{response::Json, routing::get, Router};
use clap::Parser;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::signal;
use tracing::info;

#[derive(Parser)]
#[command(name = "debug_runtime")]
#[command(about = "HTTP-controlled game runtime for LLM testing and automation")]
struct Args {
    /// Mission file to load (e.g., medsci1.mis)
    #[arg(short, long)]
    mission: Option<String>,

    /// Port to bind HTTP server to
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Enable debug physics rendering
    #[arg(long)]
    debug_physics: bool,

    /// Enable experimental features (comma-separated)
    #[arg(long)]
    experimental: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with info level by default
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug_runtime=info".into()),
        )
        .init();

    let args = Args::parse();

    info!(
        "Starting debug runtime on port {} with mission: {:?}",
        args.port,
        args.mission.as_deref().unwrap_or("none")
    );

    // Create the router with health endpoint
    let app = Router::new().route("/v1/health", get(health_check));

    // Bind to localhost only for security
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    info!("Debug runtime listening on http://{}", addr);

    // Log available endpoints
    info!("Available endpoints:");
    info!("  GET  /v1/health           - Health check and server status");
    info!("  (More endpoints coming in Phase 2)");
    info!("");
    info!("Test with: curl http://{}/v1/health", addr);

    // Start the server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Health check endpoint
async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "debug_runtime",
        "version": "0.1.0",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Wait for shutdown signal (Ctrl+C)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down gracefully...");
        },
        _ = terminate => {
            info!("Received SIGTERM, shutting down gracefully...");
        },
    }
}
