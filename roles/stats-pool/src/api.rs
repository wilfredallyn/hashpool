use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    body::Incoming, server::conn::http1, service::service_fn, Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::{convert::Infallible, sync::Arc};
use tokio::net::TcpListener;
use tracing::{error, info};

use stats_pool::db::StatsData;

pub async fn run_http_server(
    address: String,
    stats: Arc<StatsData>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&address).await?;
    info!("🌐 HTTP dashboard listening on http://{}", address);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let stats = stats.clone();

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let stats = stats.clone();
                async move { handle_request(req, stats).await }
            });

            if let Err(err) = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(io, service)
                .await
            {
                error!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn handle_request(
    req: Request<Incoming>,
    stats: Arc<StatsData>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("");

    let response = match (req.method(), path.as_str()) {
        (&Method::GET, "/api/stats") => serve_stats_json(stats.clone()).await,
        (&Method::GET, "/api/services") => serve_services_json(stats.clone()).await,
        (&Method::GET, "/api/connections") => serve_connections_json(stats.clone()).await,
        (&Method::GET, "/health") => serve_health(stats).await,
        (&Method::GET, path) if path.starts_with("/api/downstream/") && path.contains("/hashrate") => {
            let downstream_id_str = path
                .trim_start_matches("/api/downstream/")
                .trim_end_matches("/hashrate");

            if let Ok(downstream_id) = downstream_id_str.parse::<u32>() {
                let data = query_downstream_hashrate(stats.clone(), downstream_id, query).await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(data)))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Invalid downstream ID")))
                    .unwrap()
            }
        }
        (&Method::GET, "/api/hashrate") => {
            let data = query_aggregate_hashrate(stats.clone(), query).await;
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(data)))
                .unwrap()
        }
        _ => {
            let mut response = Response::new(Full::new(Bytes::from("Not Found")));
            *response.status_mut() = StatusCode::NOT_FOUND;
            response
        }
    };

    Ok(response)
}

async fn serve_stats_json(stats: Arc<StatsData>) -> Response<Full<Bytes>> {
    match stats.get_latest_snapshot() {
        Some(snapshot) => {
            let json = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"no data available"}"#)))
            .unwrap(),
    }
}

async fn serve_services_json(stats: Arc<StatsData>) -> Response<Full<Bytes>> {
    match stats.get_latest_snapshot() {
        Some(snapshot) => {
            let json =
                serde_json::to_string(&snapshot.services).unwrap_or_else(|_| "[]".to_string());
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from("[]")))
            .unwrap(),
    }
}

async fn serve_connections_json(stats: Arc<StatsData>) -> Response<Full<Bytes>> {
    match stats.get_latest_snapshot() {
        Some(snapshot) => {
            let json = serde_json::to_string(&snapshot.downstream_proxies)
                .unwrap_or_else(|_| "[]".to_string());
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from("[]")))
            .unwrap(),
    }
}

async fn serve_health(stats: Arc<StatsData>) -> Response<Full<Bytes>> {
    let stale = stats.is_stale(15);
    let status_code = if stale {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let json_response = json!({
        "healthy": !stale,
        "stale": stale
    });
    Response::builder()
        .status(status_code)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json_response.to_string())))
        .unwrap()
}

/// Parse query parameters to extract timestamp range
fn parse_timestamp_range(query: &str) -> (u64, u64) {
    let mut from = 0u64;
    let mut to = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            match key {
                "from" => {
                    if let Ok(ts) = value.parse::<u64>() {
                        from = ts;
                    }
                }
                "to" => {
                    if let Ok(ts) = value.parse::<u64>() {
                        to = ts;
                    }
                }
                _ => {}
            }
        }
    }

    (from, to)
}

async fn query_downstream_hashrate(
    stats: Arc<StatsData>,
    downstream_id: u32,
    query: &str,
) -> String {
    let (from, to) = parse_timestamp_range(query);

    match stats.query_hashrate(downstream_id, from, to).await {
        Ok(points) => {
            let data: Vec<_> = points
                .into_iter()
                .map(|p| json!({ "timestamp": p.timestamp, "hashrate_hs": p.hashrate_hs }))
                .collect();
            serde_json::to_string(&json!({ "data": data }))
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        }
        Err(e) => {
            error!("Error querying hashrate for downstream {}: {}", downstream_id, e);
            format!(r#"{{"error":"{}"}}"#, e)
        }
    }
}

async fn query_aggregate_hashrate(stats: Arc<StatsData>, query: &str) -> String {
    let (from, to) = parse_timestamp_range(query);

    match stats.query_aggregate_hashrate(from, to).await {
        Ok(points) => {
            let data: Vec<_> = points
                .into_iter()
                .map(|p| json!({ "timestamp": p.timestamp, "hashrate_hs": p.hashrate_hs }))
                .collect();
            serde_json::to_string(&json!({ "data": data }))
                .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
        }
        Err(e) => {
            error!("Error querying aggregate hashrate: {}", e);
            format!(r#"{{"error":"{}"}}"#, e)
        }
    }
}
