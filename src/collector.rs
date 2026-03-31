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

/// Spawns the background collector thread.
///
/// The thread takes exclusive ownership of the profilers and scheduler,
/// collecting metrics in a loop and publishing the rendered output to
/// `snapshot`.
pub fn spawn(
    mut profilers: Vec<Box<dyn Profiler + Send>>,
    scheduler: Box<dyn HpcScheduler + Send>,
    snapshot: Arc<ArcSwap<String>>,
    interval: Duration,
) {
    thread::spawn(move || loop {
        let output = collect(&mut profilers, &*scheduler);
        snapshot.store(Arc::new(output));
        thread::sleep(interval);
    });
}

/// Runs a single collection pass across all profilers.
///
/// Failures at any stage are logged and skipped rather than propagated,
/// so partial metrics are still reported when a single profiler fails.
fn collect(profilers: &mut [Box<dyn Profiler + Send>], scheduler: &dyn HpcScheduler) -> String {
    let processes = scheduler.get_processes().unwrap_or_else(|e| {
        warn!("failed to fetch job pids: {e}");
        Vec::new()
    });

    let mut output = String::new();
    for profiler in profilers.iter_mut() {
        match profiler.collect_metrics(&processes) {
            Ok(metrics) => {
                for m in &metrics {
                    output.push_str(&m.to_prometheus());
                    output.push('\n');
                }
            }
            Err(e) => {
                error!("failed to collect metrics: {e}");
            }
        }
    }

    output
}
