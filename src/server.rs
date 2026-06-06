use crate::config::Config;
use crate::db::{query_state, KaraokeState};
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tokio::time::{interval, Duration};
use tower_http::cors::CorsLayer;

type SharedState = Arc<RwLock<Option<KaraokeState>>>;

const TICKER_HTML: &str = include_str!("static/ticker.html");
const LIST_HTML: &str = include_str!("static/list.html");

pub async fn run(cfg: Config) -> Result<()> {
    let shared: SharedState = Arc::new(RwLock::new(None));

    // Background task: poll the SQLite database on a fixed interval.
    if let Some(data_dir) = cfg.data_dir.clone() {
        let poll_shared = shared.clone();
        let poll_interval_ms = cfg.ticker.poll_interval_ms;
        let singer_count = cfg.ticker.singer_count;

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(poll_interval_ms));
            loop {
                ticker.tick().await;
                match query_state(&data_dir, singer_count) {
                    Ok(state) => {
                        *poll_shared.write().unwrap() = Some(state);
                    }
                    Err(e) => {
                        tracing::warn!("DB query error: {}", e);
                    }
                }
            }
        });
    } else {
        tracing::warn!(
            "No OpenKJ data directory configured. \
            Set data_dir in the config file or pass --data-dir."
        );
    }

    let app = Router::new()
        .route("/", get(list_handler))
        .route("/ticker", get(ticker_handler))
        .route("/api/state", get(api_state_handler))
        .layer(CorsLayer::permissive())
        .with_state(shared);

    let addr: SocketAddr = format!("{}:{}", cfg.server.bind_address, cfg.server.port).parse()?;

    print_startup_info(cfg.server.port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn print_startup_info(port: u16) {
    let local_ip = get_local_ip().unwrap_or_else(|| "<your-machine-ip>".to_string());

    println!();
    println!("openkj-ticker is running");
    println!("────────────────────────────────────────────────");
    println!("  Dashboard :  http://localhost:{port}/");
    println!("  OBS Ticker:  http://localhost:{port}/ticker");
    println!("  JSON API  :  http://localhost:{port}/api/state");
    println!();
    println!("  From other machines on your network:");
    println!("  Dashboard :  http://{local_ip}:{port}/");
    println!("  OBS Ticker:  http://{local_ip}:{port}/ticker");
    println!("────────────────────────────────────────────────");
    println!("Press Ctrl+C to stop.");
    println!();
}

/// Discover the outbound local IP without sending any packets.
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip().to_string())
}

async fn ticker_handler() -> impl IntoResponse {
    Html(TICKER_HTML)
}

async fn list_handler() -> impl IntoResponse {
    Html(LIST_HTML)
}

async fn api_state_handler(State(state): State<SharedState>) -> impl IntoResponse {
    match state.read().unwrap().clone() {
        Some(s) => Json(s).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "message": "OpenKJ data not yet loaded or data directory not found"
            })),
        )
            .into_response(),
    }
}
