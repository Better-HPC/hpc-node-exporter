//! Background metric collection.
//!
//! Metrics collection is executed in a background thread, rendered
//! into Prometheus format, and published to an [`ArcSwap`] for
//! consumption by the rest of the application.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use arc_swap::ArcSwap;
use log::{error, warn};

use crate::profilers::Profiler;
use crate::schedulers::HpcScheduler;

/// Spawn the background collector thread.
///
/// The thread takes exclusive ownership of the profilers and scheduler,
/// collecting metrics in a loop and publishing the rendered output to
/// the shared `snapshot`. The thread runs for the lifetime of the process.
///
/// # Arguments
///
/// * `profilers` - The enabled profiler instances.
/// * `scheduler` - The HPC scheduler used to discover active jobs.
/// * `snapshot` - The shared snapshot that HTTP handlers read from.
/// * `interval` - How often to collect and publish metrics.
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

/// Run a single collection pass across all profilers.
///
/// Fetches active processes from the scheduler, then collects metrics
/// from each profiler and renders them into a single Prometheus-format
/// string. Failures at any stage are logged and skipped rather than
/// propagated, ensuring that one broken profiler or a transient scheduler
/// error doesn't take down the collector loop.
///
/// # Arguments
///
/// * `profilers` - The profiler instances to collect from.
/// * `scheduler` - The HPC scheduler used to discover active job PIDs.
///
/// # Returns
///
/// A Prometheus text exposition format string containing the rendered
/// metrics from all profilers.
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
