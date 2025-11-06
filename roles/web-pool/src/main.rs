use stats::stats_adapter::PoolSnapshot;
use std::{sync::Arc, time::Duration};
use tokio::time;
use tracing::{error, info};
use tracing_subscriber;

use web_pool::{config::Config, SnapshotStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
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

    info!("Starting web-pool service");
    info!("Stats pool URL: {}", config.stats_pool_url);
    info!("Web server address: {}", config.web_server_address);
    info!(
        "Stats polling interval: {} seconds",
        config.stats_poll_interval_secs
    );
    info!(
        "Client polling interval: {} seconds",
        config.client_poll_interval_secs
    );

    // Create shared snapshot storage
    let storage = Arc::new(SnapshotStorage::new());

    // Spawn polling loop
    let storage_clone = storage.clone();
    let stats_pool_url = config.stats_pool_url.clone();
    let poll_interval = config.stats_poll_interval_secs;
    let request_timeout = config.request_timeout_secs;
    let pool_idle_timeout = config.pool_idle_timeout_secs;
    tokio::spawn(async move {
        poll_stats_pool(
            storage_clone,
            stats_pool_url,
            poll_interval,
            request_timeout,
            pool_idle_timeout,
        )
        .await;
    });

    // Start HTTP server with client polling interval
    start_web_server(
        config.web_server_address,
        storage,
        config.client_poll_interval_secs,
        Some(config.stats_pool_url.clone()),
    )
    .await?;

    Ok(())
}

async fn poll_stats_pool(
    storage: Arc<SnapshotStorage>,
    stats_pool_url: String,
    poll_interval_secs: u64,
    request_timeout_secs: u64,
    pool_idle_timeout_secs: u64,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(request_timeout_secs))
        .pool_idle_timeout(Duration::from_secs(pool_idle_timeout_secs))
        .pool_max_idle_per_host(1)
        .build()
        .unwrap();
    let mut interval = time::interval(Duration::from_secs(poll_interval_secs));
    let mut last_success = false;

    loop {
        interval.tick().await;

        match client
            .get(format!("{}/api/stats", stats_pool_url))
            .send()
            .await
        {
            Ok(response) => match response.json::<PoolSnapshot>().await {
                Ok(snapshot) => {
                    if !last_success {
                        info!("Successfully fetched snapshot from stats-pool");
                        last_success = true;
                    }
                    storage.update(snapshot);
                }
                Err(e) => {
                    if last_success {
                        error!("Failed to parse snapshot JSON: {}", e);
                        last_success = false;
                    }
                }
            },
            Err(e) => {
                if last_success {
                    error!("Failed to fetch from stats-pool: {}", e);
                    last_success = false;
                }
            }
        }
    }
}

async fn start_web_server(
    address: String,
    storage: Arc<SnapshotStorage>,
    client_poll_interval_secs: u64,
    stats_pool_url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    web_pool::web::run_http_server(address, storage, client_poll_interval_secs, stats_pool_url).await
}
