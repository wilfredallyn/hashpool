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

use crate::db::StatsData;

pub async fn run_http_server(
    address: String,
    db: Arc<StatsData>,
    redact_ip: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&address).await?;
    info!("🌐 HTTP API listening on http://{}", address);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let db = db.clone();

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let db = db.clone();
                async move { handle_request(req, db, redact_ip).await }
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
    db: Arc<StatsData>,
    redact_ip: bool,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("");

    let response = match (req.method(), path.as_str()) {
        (&Method::GET, "/api/stats") => {
            let snapshot = get_snapshot(db.clone()).await;
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(snapshot)))
        }
        (&Method::GET, "/api/miners") => {
            let stats = get_miner_stats(db, redact_ip).await;
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(stats.to_string())))
        }
        (&Method::GET, path) if path.starts_with("/api/downstream/") && path.contains("/hashrate") => {
            let downstream_id_str = path
                .trim_start_matches("/api/downstream/")
                .trim_end_matches("/hashrate");

            if let Ok(downstream_id) = downstream_id_str.parse::<u32>() {
                let data = query_downstream_hashrate(db.clone(), downstream_id, query).await;
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(data)))
            } else {
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Invalid downstream ID")))
            }
        }
        (&Method::GET, "/api/hashrate") => {
            let data = query_aggregate_hashrate(db.clone(), query).await;
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(data)))
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found"))),
    };

    Ok(response.unwrap_or_else(|e| {
        error!("Error building response: {:?}", e);
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("Internal Server Error")))
            .unwrap()
    }))
}

async fn get_snapshot(db: Arc<StatsData>) -> String {
    match db.get_latest_snapshot() {
        Some(snapshot) => serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string()),
        None => r#"{"error":"no data available"}"#.to_string(),
    }
}

async fn get_miner_stats(db: Arc<StatsData>, redact_ip: bool) -> serde_json::Value {
    match db.get_latest_snapshot() {
        Some(snapshot) => {
            let miners: Vec<_> = snapshot
                .downstream_miners
                .into_iter()
                .map(|mut m| {
                    if redact_ip {
                        m.address = m.address.split(':').next().unwrap_or("").to_string() + ":****";
                    }
                    m
                })
                .collect();
            json!({ "miners": miners })
        }
        None => json!({ "miners": [] }),
    }
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
    db: Arc<StatsData>,
    downstream_id: u32,
    query: &str,
) -> String {
    let (from, to) = parse_timestamp_range(query);

    match db.query_hashrate(downstream_id, from, to).await {
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

async fn query_aggregate_hashrate(db: Arc<StatsData>, query: &str) -> String {
    let (from, to) = parse_timestamp_range(query);

    match db.query_aggregate_hashrate(from, to).await {
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
