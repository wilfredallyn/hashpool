use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tracing::info;

use crate::SnapshotStorage;
use web_assets::icons::{nav_icon_css, pickaxe_favicon_inline_svg};
use web_utils::format_elapsed_time;

static DASHBOARD_PAGE_HTML: OnceLock<String> = OnceLock::new();
static CLIENT_POLL_INTERVAL_SECS: OnceLock<u64> = OnceLock::new();

const DASHBOARD_PAGE_TEMPLATE: &str = include_str!("../templates/dashboard.html");

pub async fn run_http_server(
    address: String,
    storage: Arc<SnapshotStorage>,
    client_poll_interval_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Store the polling interval for use in dashboard_page
    let _ = CLIENT_POLL_INTERVAL_SECS.set(client_poll_interval_secs);

    let app = Router::new()
        .route("/favicon.ico", get(serve_favicon))
        .route("/favicon.svg", get(serve_favicon))
        .route("/", get(dashboard_page_handler))
        .route("/api/stats", get(api_stats_handler))
        .route("/api/services", get(api_services_handler))
        .route("/api/connections", get(api_connections_handler))
        .route("/health", get(health_handler))
        .with_state(storage);

    let listener = tokio::net::TcpListener::bind(&address).await?;
    info!("🌐 Web pool listening on http://{}", address);
    info!(
        "Client polling interval: {} seconds",
        client_poll_interval_secs
    );

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

async fn dashboard_page_handler() -> impl IntoResponse {
    let interval_ms = CLIENT_POLL_INTERVAL_SECS.get().copied().unwrap_or(3) * 1000;
    let html = DASHBOARD_PAGE_HTML.get_or_init(|| {
        DASHBOARD_PAGE_TEMPLATE
            .replace("/* {{NAV_ICON_CSS}} */", nav_icon_css())
            .replace("{client_poll_interval_ms}", &interval_ms.to_string())
    });
    Html(html.clone())
}

async fn api_stats_handler(State(storage): State<Arc<SnapshotStorage>>) -> impl IntoResponse {
    let stats = get_pool_stats(storage);
    Json(stats)
}

async fn api_services_handler(State(storage): State<Arc<SnapshotStorage>>) -> impl IntoResponse {
    let services = get_services(storage);
    Json(services)
}

async fn api_connections_handler(State(storage): State<Arc<SnapshotStorage>>) -> impl IntoResponse {
    let connections = get_connections(storage);
    Json(connections)
}

async fn health_handler(State(storage): State<Arc<SnapshotStorage>>) -> impl IntoResponse {
    let stale = storage.is_stale(15);
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

fn get_pool_stats(storage: Arc<SnapshotStorage>) -> serde_json::Value {
    match storage.get() {
        Some(snapshot) => {
            json!({
                "listen_address": snapshot.listen_address,
                "services": snapshot.services,
                "downstream_proxies": snapshot.downstream_proxies,
                "timestamp": snapshot.timestamp
            })
        }
        None => {
            json!({
                "listen_address": "",
                "services": [],
                "downstream_proxies": [],
                "timestamp": 0
            })
        }
    }
}

fn get_services(storage: Arc<SnapshotStorage>) -> serde_json::Value {
    match storage.get() {
        Some(snapshot) => {
            let services: Vec<serde_json::Value> = snapshot
                .services
                .iter()
                .map(|s| {
                    json!({
                        "service_type": format!("{:?}", s.service_type),
                        "address": s.address
                    })
                })
                .collect();
            json!({ "services": services })
        }
        None => json!({ "services": [] }),
    }
}

fn get_connections(storage: Arc<SnapshotStorage>) -> serde_json::Value {
    match storage.get() {
        Some(snapshot) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let proxies: Vec<serde_json::Value> = snapshot
                .downstream_proxies
                .iter()
                .map(|p| {
                    let last_share = p.last_share_at.map(|ts| format_elapsed_time(now, ts));

                    json!({
                        "id": p.id,
                        "address": p.address,
                        "channels": p.channels,
                        "shares_submitted": p.shares_submitted,
                        "quotes_created": p.quotes_created,
                        "ehash_mined": p.ehash_mined,
                        "last_share_at": last_share,
                        "work_selection": p.work_selection
                    })
                })
                .collect();
            json!({ "proxies": proxies })
        }
        None => json!({ "proxies": [] }),
    }
}
