//! Background metric collection.
//!
//! Metrics are collected in a background thread, rendered into Prometheus
//! text exposition format, and published to an [`ArcSwap`] for lock-free
//! reads by other application layers.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use bytes::Bytes;
use log::{error, warn};
use tokio::sync::watch;

use crate::profilers::Profiler;
use crate::schedulers::HpcScheduler;

/// Spawns a background thread that collects metrics on a fixed `interval`.
///
/// Each iteration calls every [`Profiler`] in `profilers`, renders the
/// results into Prometheus text format, and publishes results to the
/// `snapshot` memory buffer.
///
/// If `notify` is provided, a signal is sent after each successful snapshot
/// store so that other components (e.g. the push subsystem) can react to
/// new data without polling.
///
/// Panics within a collection pass are caught and logged; the previous
/// snapshot is retained until the next successful iteration.
pub fn run(
    mut profilers: Vec<Box<dyn Profiler + Send>>,
    scheduler: Box<dyn HpcScheduler + Send>,
    snapshot: Arc<ArcSwap<Bytes>>,
    interval: Duration,
    notify: Option<watch::Sender<()>>,
) {
    thread::spawn(move || {
        // An internal buffer used to store metrics while they are being collected
        let mut buf = bytes::BytesMut::new();

        loop {
            // Determine when the next collection is supposed to start
            let deadline = Instant::now() + interval;

            // Execute the current metrics collection
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                collect_into_buffer(&mut profilers, &*scheduler, &mut buf);
            }));

            // On failure - clear the buffer
            if let Err(e) = result {
                error!("collector panicked, skipping iteration: {:?}", e);
                buf.clear();

            // On success - move data from the internal buffer to the snapshot
            // Reallocate space for the internal buffer and notify any listeners
            } else {
                let last_len = buf.len();
                snapshot.store(Arc::new(buf.split().freeze()));
                buf.reserve(last_len);

                if let Some(tx) = &notify {
                    let _ = tx.send(());
                }
            }

            // Sleep until the next metrics collection is scheduled to start
            // Begin immediately if the previous collection overran the deadline
            match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) => thread::sleep(remaining),
                None => warn!(
                    "collection pass exceeded the configured interval ({interval:?}); \
                     consider increasing --interval"
                ),
            }
        }
    });
}

/// Collect metrics from all profilers and store results in a buffer.
///
/// Failures at any stage are logged and skipped rather than propagated,
/// so partial metrics are still reported when a single profiler fails.
fn collect_into_buffer(
    profilers: &mut [Box<dyn Profiler + Send>],
    scheduler: &dyn HpcScheduler,
    buf: &mut bytes::BytesMut,
) {
    let processes = scheduler.get_processes().unwrap_or_else(|e| {
        warn!("failed to fetch job pids: {e}");
        Vec::new()
    });

    buf.clear(); // Clear existing metrics from the memory buffer

    // Populate the buffer with metrics from each profiler
    for profiler in profilers.iter_mut() {
        match profiler.collect_metrics(&processes) {
            Ok(families) => {
                for family in &families {
                    let rendered = family.to_prometheus();
                    if !rendered.is_empty() {
                        buf.extend_from_slice(rendered.as_bytes());
                        buf.extend_from_slice(b"\n");
                    }
                }
            }
            Err(e) => {
                error!("failed to collect metrics: {e}");
            }
        }
    }
}
