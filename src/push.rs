//! Background push delivery of metrics to a remote endpoint.
//!
//! This module delivers each freshly-collected snapshot to the remote
//! endpoint via HTTP POST. Collection and scraping are entirely
//! unaffected by the health or responsiveness of the remote endpoint.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use bytes::Bytes;
use log::{error, warn};
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::timeout;

/// The content type expected by VictoriaMetrics and Prometheus-compatible
/// remote write endpoints for text exposition format payloads.
const CONTENT_TYPE: &str = "text/plain";

/// Default number of snapshots to buffer before dropping.
pub const DEFAULT_BUFFER_SIZE: usize = 30;

/// Default number of concurrent POST workers.
pub const DEFAULT_WORKER_COUNT: usize = 4;

/// Starts the push subsystem.
///
/// Spawns a push loop task and a pool of worker tasks. Returns a
/// [`watch::Sender`] that the collector should signal after each new snapshot
/// is stored. If the sender is dropped, the push loop exits cleanly.
///
/// This function returns immediately — all work happens in the background.
pub fn run(
    snapshot: Arc<ArcSwap<Bytes>>,
    url: String,
    buffer_size: usize,
    worker_count: usize,
    post_timeout: Duration,
) -> watch::Sender<()> {
    let client = reqwest::Client::new();
    let (tx, rx) = mpsc::channel::<Bytes>(buffer_size);
    let (notify_tx, notify_rx) = watch::channel(());

    // Wrap the receiver in an Arc<Mutex> so it can be shared across workers.
    let rx = Arc::new(Mutex::new(rx));

    // Spawn the push loop task.
    tokio::spawn(run_push_loop(snapshot, notify_rx, tx));

    // Spawn the worker pool.
    for _ in 0..worker_count {
        tokio::spawn(run_worker(
            Arc::clone(&rx),
            client.clone(),
            url.clone(),
            post_timeout,
        ));
    }

    notify_tx
}

/// Watches for new snapshots and forwards them into the worker channel.
///
/// Loads the current snapshot from the [`ArcSwap`] on each notification and
/// attempts to send it into the bounded channel. If the channel is full the
/// snapshot is dropped and a warning is logged — this is the explicit and
/// intentional point of data loss under overload.
async fn run_push_loop(
    snapshot: Arc<ArcSwap<Bytes>>,
    mut notify: watch::Receiver<()>,
    tx: mpsc::Sender<Bytes>,
) {
    loop {
        // Wait for the collector to signal a new snapshot is available.
        if notify.changed().await.is_err() {
            // The sender has been dropped, meaning the collector has shut down.
            // Nothing left to do.
            return;
        }

        let bytes = snapshot.load().as_ref().clone();

        // Skip empty snapshots that may appear before the first collection pass.
        if bytes.is_empty() {
            continue;
        }

        // try_send returns an error if the channel is full (TrySendError::Full)
        // or if all receivers have been dropped (TrySendError::Closed). In both
        // cases we log and move on rather than blocking or panicking.
        if let Err(e) = tx.try_send(bytes) {
            warn!("push buffer full, dropping snapshot: {e}");
        }
    }
}

/// Drains the worker channel and POSTs each snapshot to the remote endpoint.
///
/// Each POST is bounded by `post_timeout`. Timeouts and HTTP errors are both
/// logged and skipped — the worker always returns to draining the channel
/// regardless of the outcome.
async fn run_worker(
    rx: Arc<Mutex<mpsc::Receiver<Bytes>>>,
    client: reqwest::Client,
    url: String,
    post_timeout: Duration,
) {
    loop {
        // Hold the lock only long enough to receive one snapshot, then release
        // it so other workers can immediately take the next item.
        let bytes = {
            let mut rx = rx.lock().await;
            match rx.recv().await {
                Some(b) => b,
                // Channel closed, collector and push loop have both shut down.
                None => return,
            }
        };

        let result = timeout(
            post_timeout,
            client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE)
                .body(bytes)
                .send(),
        )
        .await;

        match result {
            Err(_elapsed) => {
                error!("push timed out after {post_timeout:?}");
            }
            Ok(Err(e)) => {
                error!("push request failed: {e}");
            }
            Ok(Ok(resp)) if !resp.status().is_success() => {
                error!("push rejected with status {}", resp.status());
            }
            Ok(Ok(_)) => {}
        }
    }
}
