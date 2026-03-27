//! HTTP API server for Prometheus metric scraping.
//!
//! Exposes a `/metrics` endpoint that returns pre-collected telemetry in
//! Prometheus text exposition format. Metrics are collected on a background
//! thread (see [`crate::collector`]) and published via an [`ArcSwap`] snapshot
//! that handlers read with zero contention.

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::extract::State;
use axum::routing::get;
use axum::Router;
use log::info;
use tokio::net::TcpListener;

use crate::collector::MetricsStore;

/// GET /metrics — return the latest pre-collected metrics snapshot.
///
/// Loads the current snapshot from application [`ArcSwap`] and clones
/// the inner string. The clone is cheap relative to a full collection
/// pass, and [`ArcSwap::load`] is lock-free, so concurrent scrapes do
/// not block each other or the collector thread.
///
/// # Returns
///
/// The latest Prometheus-format metrics string.
async fn metrics_handler(State(state): State<&MetricsStore>) -> String {
    state.snapshot.load().as_ref().clone()
}

/// Build the Axum router with shared application state.
///
/// # Arguments
///
/// * `state` - A `'static` reference to the shared [`MetricsStore`].
///
/// # Returns
///
/// A configured [`Router`] with the `/metrics` route registered.
fn build_router(state: &'static MetricsStore) -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/metrics/", get(metrics_handler))
        .with_state(state)
}

/// Start the HTTP server on the given host and port.
///
/// The server reads from a shared [`ArcSwap<String>`] snapshot that is
/// populated by the collector thread. This function leaks the [`MetricsStore`]
/// into a `&'static` reference so it can be shared across Axum handlers
/// without additional `Arc` overhead.
///
/// # Arguments
///
/// * `host` - The network interface to bind to (e.g., `"127.0.0.1"`).
/// * `port` - The TCP port to listen on.
/// * `snapshot` - The shared snapshot that the collector thread writes to.
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
    let state: &'static MetricsStore = Box::leak(Box::new(MetricsStore { snapshot }));

    let router = build_router(state);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;

    info!("listening on http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}
