use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};
use tracing::{error, info};

mod api;
mod config;

use config::Config;
use stats_pool::{db::StatsData, stats_handler::StatsHandler};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args()?;

    // Setup tracing with optional file output
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt()
        .with_env_filter(env_filter);

    if let Some(log_file) = &config.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| format!("Failed to open log file {}: {}", log_file, e))?;
        fmt_layer.with_writer(std::sync::Arc::new(file)).init();
    } else {
        fmt_layer.init();
    }

    info!("Starting pool-stats service");
    info!("TCP server: {}", config.tcp_address);
    info!("HTTP server: {}", config.http_address);

    let stats = Arc::new(StatsData::new());

    // Initialize metrics storage with SQLite backend
    if let Err(e) = stats.init_metrics_storage(Some(&config.metrics_db_path)).await {
        error!("Failed to initialize metrics storage: {}", e);
    } else {
        info!("Metrics storage initialized at {}", config.metrics_db_path);
    }

    let tcp_listener = TcpListener::bind(&config.tcp_address).await?;
    info!("TCP server listening on {}", config.tcp_address);

    // HTTP API server exposes snapshots to web services
    let http_address = config.http_address.clone();
    let stats_for_http = stats.clone();
    tokio::spawn(async move {
        if let Err(e) = api::run_http_server(http_address, stats_for_http).await {
            error!("HTTP server error: {}", e);
        }
    });

    loop {
        match tcp_listener.accept().await {
            Ok((stream, addr)) => {
                info!("New pool connection from {}", addr);
                let stats_clone = stats.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_pool_connection(stream, addr, stats_clone).await {
                        error!("Error handling pool connection from {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}

async fn handle_pool_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    stats: Arc<StatsData>,
) -> Result<(), Box<dyn std::error::Error>> {
    let handler = StatsHandler::new(stats);
    let mut buffer = vec![0u8; 8192];
    let mut leftover = Vec::new();

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                info!("Pool connection from {} closed", addr);
                break;
            }
            Ok(n) => {
                leftover.extend_from_slice(&buffer[..n]);

                while let Some(newline_pos) = leftover.iter().position(|&b| b == b'\n') {
                    let line = &leftover[..newline_pos];

                    if !line.is_empty() {
                        if let Err(e) = handler.handle_message(line).await {
                            error!("Error processing message from {}: {}", addr, e);
                        }
                    }

                    leftover.drain(..=newline_pos);
                }
            }
            Err(e) => {
                error!("Error reading from {}: {}", addr, e);
                break;
            }
        }
    }

    Ok(())
}
