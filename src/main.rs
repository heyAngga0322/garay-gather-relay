mod app;
mod config;
mod metrics;
mod signaling;
mod ui;
mod webrtc;

use config::ServerConfig;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::signal::ctrl_c;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = ServerConfig::load()?;

    // Initialize structured logging
    if config.log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
            .init();
    }

    // Initialize Prometheus metrics exporter
    let (recorder, exporter) = PrometheusBuilder::new()
        .build()
        .expect("failed to build Prometheus exporter");
    let metrics_handle = recorder.handle();

    // Spawn the exporter task
    tokio::spawn(exporter);

    let turn_config = config.turn_config_json();
    let app = app::create_app(metrics_handle, config.max_peers_per_room, turn_config).await;

    let listener = tokio::net::TcpListener::bind(config.bind_address)
        .await
        .expect("failed to bind to address");

    info!(address = %listener.local_addr().unwrap(), "WebRTC signaling server starting");

    // Graceful shutdown with connection draining
    let shutdown = async {
        ctrl_c().await.expect("failed to install Ctrl+C handler");
        info!("Shutdown signal received, draining connections...");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .expect("server failed");

    Ok(())
}
