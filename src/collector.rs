//! Background metric collection.
//!
//! Metrics are collected in a background thread, rendered into Prometheus
//! text exposition format, and published to an [`ArcSwap`] for lock-free
//! reads by other application layers.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use arc_swap::ArcSwap;
use log::{error, warn};

use crate::profilers::Profiler;
use crate::schedulers::HpcScheduler;
use bytes::Bytes;

// Note: buf is now BytesMut, snapshot stores Bytes
pub fn spawn(
    mut profilers: Vec<Box<dyn Profiler + Send>>,
    scheduler: Box<dyn HpcScheduler + Send>,
    snapshot: Arc<ArcSwap<Bytes>>,
    interval: Duration,
) {
    thread::spawn(move || {
        let mut buf = bytes::BytesMut::new();
        loop {
            collect_into_buffer(&mut profilers, &*scheduler, &mut buf);
            snapshot.store(Arc::new(buf.split().freeze()));
            thread::sleep(interval);
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

    // Clear existing metrics from the memory buffer
    buf.clear();
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
