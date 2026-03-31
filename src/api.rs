//! HTTP API server for Prometheus metric scraping.
//!
//! Exposes a `/metrics` endpoint that returns pre-collected telemetry in
//! Prometheus text exposition format. Metrics are published via an
//! [`ArcSwap`] snapshot that handlers read with zero contention.

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use log::info;
use tokio::net::TcpListener;

/// Returns an empty `200 OK` for health checks.
async fn status_handler() -> StatusCode {
    StatusCode::OK
}

/// Returns the latest Prometheus-format metrics snapshot.
async fn metrics_handler(State(snapshot): State<&'static ArcSwap<String>>) -> String {
    snapshot.load().as_ref().clone()
}

/// Builds the Axum router with shared application state.
fn build_router(snapshot: &'static ArcSwap<String>) -> Router {
    Router::new()
        .route("/", get(status_handler))
        .route("/metrics", get(metrics_handler))
        .route("/metrics/", get(metrics_handler))
        .with_state(snapshot)
}

/// Starts the HTTP server on the given `host` and `port`.
///
/// Reads rendered Prometheus metrics from `snapshot`, which is populated
/// by the background collector thread.
///
/// # Errors
///
/// Returns an error if the TCP listener fails to bind or the server
/// encounters a fatal I/O error.
pub async fn serve(
    host: &str,
    port: u16,
    snapshot: Arc<ArcSwap<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot: &'static ArcSwap<String> = {
        let leaked: &'static Arc<ArcSwap<String>> = Box::leak(Box::new(snapshot));
        leaked.as_ref()
    };

    let router = build_router(snapshot);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;

    info!("listening on http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}
