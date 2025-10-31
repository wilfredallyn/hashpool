use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tracing::{error, info};

use crate::SnapshotStorage;
use web_assets::icons::{nav_icon_css, pickaxe_favicon_inline_svg};
use web_utils::{format_elapsed_time, format_hashrate};

static MINERS_PAGE_HTML: OnceLock<String> = OnceLock::new();

const WALLET_PAGE_TEMPLATE: &str = include_str!("../templates/wallet.html");
const MINERS_PAGE_TEMPLATE: &str = include_str!("../templates/miners.html");
const POOL_PAGE_TEMPLATE: &str = include_str!("../templates/pool.html");

pub struct AppState {
    pub storage: Arc<SnapshotStorage>,
    pub http_client: reqwest::Client,
    pub faucet_enabled: bool,
    pub faucet_url: Option<String>,
    pub downstream_address: String,
    pub downstream_port: u16,
    pub upstream_address: String,
    pub upstream_port: u16,
    pub client_poll_interval_secs: u64,
}

pub async fn run_http_server(
    address: String,
    storage: Arc<SnapshotStorage>,
    faucet_enabled: bool,
    faucet_url: Option<String>,
    downstream_address: String,
    downstream_port: u16,
    upstream_address: String,
    upstream_port: u16,
    client_poll_interval_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = reqwest::Client::new();

    let state = AppState {
        storage,
        http_client,
        faucet_enabled,
        faucet_url,
        downstream_address,
        downstream_port,
        upstream_address,
        upstream_port,
        client_poll_interval_secs,
    };

    let app = Router::new()
        .route("/favicon.ico", get(serve_favicon))
        .route("/favicon.svg", get(serve_favicon))
        .route("/", get(wallet_page_handler))
        .route("/miners", get(miners_page_handler))
        .route("/pool", get(pool_page_handler))
        .route("/api/miners", get(api_miners_handler))
        .route("/api/pool", get(api_pool_handler))
        .route("/balance", get(balance_handler))
        .route("/health", get(health_handler))
        .route("/mint/tokens", post(mint_tokens_handler))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(&address).await?;
    info!("🌐 Web proxy listening on http://{}", address);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_favicon() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "image/svg+xml")],
        pickaxe_favicon_inline_svg(),
    )
}

async fn wallet_page_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let html = WALLET_PAGE_TEMPLATE.replace("/* {{NAV_ICON_CSS}} */", nav_icon_css());

    let html = if !state.faucet_enabled {
        // Remove mint button if faucet is disabled
        html.replace(
            r#"<button class="mint-button" id="drip-btn" onclick="requestDrip()">Mint</button>"#,
            "",
        )
    } else {
        html
    };

    Html(html)
}

async fn miners_page_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let html = MINERS_PAGE_HTML.get_or_init(|| {
        MINERS_PAGE_TEMPLATE.replace("/* {{NAV_ICON_CSS}} */", nav_icon_css())
    });

    let formatted_html = html
        .replace("{downstream_address}", &state.downstream_address)
        .replace("{downstream_port}", &state.downstream_port.to_string());

    Html(formatted_html)
}

async fn pool_page_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let html = POOL_PAGE_TEMPLATE.replace("/* {{NAV_ICON_CSS}} */", nav_icon_css());

    // Convert seconds to milliseconds for JavaScript setInterval
    let client_poll_interval_ms = state.client_poll_interval_secs * 1000;

    let formatted_html = html
        .replace("{upstream_address}", &state.upstream_address)
        .replace("{upstream_port}", &state.upstream_port.to_string())
        .replace(
            "{client_poll_interval_ms}",
            &client_poll_interval_ms.to_string(),
        );

    Html(formatted_html)
}

async fn api_miners_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = get_miner_stats(&state.storage);
    Json(stats)
}

async fn api_pool_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pool_info = get_pool_info(&state.storage);
    Json(pool_info)
}

async fn balance_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let balance = get_wallet_balance(&state.storage);
    let json_response = json!({
        "balance": format!("{} ehash", balance),
        "balance_raw": balance,
        "unit": "HASH"
    });
    Json(json_response)
}

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stale = state.storage.is_stale(15);
    let status_code = if stale {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let json_response = json!({
        "healthy": !stale,
        "stale": stale
    });
    (status_code, Json(json_response))
}

async fn mint_tokens_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.faucet_enabled {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"Faucet is disabled"})),
        );
    }

    let Some(faucet_url) = &state.faucet_url else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"Faucet URL not configured"})),
        );
    };

    // Proxy mint request to translator's faucet API
    let translator_faucet_url = format!("{}/mint/tokens", faucet_url);

    match state
        .http_client
        .post(&translator_faucet_url)
        .header("content-length", "0")
        .body("")
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    let status_code = StatusCode::from_u16(status.as_u16())
                        .unwrap_or_else(|_| {
                            error!("Invalid status code from translator: {}", status);
                            StatusCode::INTERNAL_SERVER_ERROR
                        });
                    // Parse body as JSON if possible, otherwise wrap as raw text
                    let json_body = serde_json::from_str::<serde_json::Value>(&body)
                        .unwrap_or_else(|_| json!({"response": body}));
                    (status_code, Json(json_body))
                }
                Err(e) => {
                    error!("Failed to read response from translator: {}", e);
                    let json_response = json!({
                        "success": false,
                        "error": "Failed to read mint response"
                    });
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json_response))
                }
            }
        }
        Err(e) => {
            error!("Failed to proxy mint request to translator: {}", e);
            let json_response = json!({
                "success": false,
                "error": format!("Faucet unavailable: {}", e)
            });
            (StatusCode::SERVICE_UNAVAILABLE, Json(json_response))
        }
    }
}

fn get_wallet_balance(storage: &Arc<SnapshotStorage>) -> u64 {
    match storage.get() {
        Some(snapshot) => snapshot.ehash_balance,
        None => 0,
    }
}

fn get_pool_info(storage: &Arc<SnapshotStorage>) -> serde_json::Value {
    match storage.get() {
        Some(snapshot) => {
            json!({
                "blockchain_network": snapshot.blockchain_network,
                "upstream_pool": snapshot.upstream_pool,
                "connected": snapshot.upstream_pool.is_some()
            })
        }
        None => {
            json!({
                "blockchain_network": "unknown",
                "upstream_pool": null,
                "connected": false
            })
        }
    }
}

fn get_miner_stats(storage: &Arc<SnapshotStorage>) -> serde_json::Value {
    let snapshot = match storage.get() {
        Some(snapshot) => snapshot,
        None => {
            return json!({
                "total_miners": 0,
                "total_hashrate": "0 H/s",
                "total_shares": 0,
                "miners": []
            })
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let total_miners = snapshot.downstream_miners.len();
    let total_shares: u64 = snapshot
        .downstream_miners
        .iter()
        .map(|m| m.shares_submitted)
        .sum();
    let total_hashrate_raw: f64 = snapshot.downstream_miners.iter().map(|m| m.hashrate).sum();

    let total_hashrate = format_hashrate(total_hashrate_raw);

    let miners: Vec<serde_json::Value> = snapshot
        .downstream_miners
        .iter()
        .map(|m| {
            let connected_time = format_elapsed_time(now, m.connected_at);

            json!({
                "name": m.name,
                "id": m.id,
                "address": m.address,
                "hashrate": format_hashrate(m.hashrate),
                "shares": m.shares_submitted,
                "connected_time": connected_time
            })
        })
        .collect();

    json!({
        "total_miners": total_miners,
        "total_hashrate": total_hashrate,
        "total_shares": total_shares,
        "miners": miners
    })
}
