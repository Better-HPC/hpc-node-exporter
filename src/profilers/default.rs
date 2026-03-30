//! Default profiler that is always enabled.
//!
//! Reports scheduler-level metrics derived entirely from the process
//! list provided by the HPC scheduler.

use std::collections::HashSet;
use std::error::Error;

use crate::profilers::{Metric, Profiler, HOSTNAME};
use crate::schedulers::HpcProcess;

/// A [`Profiler`] that reports baseline HPC scheduler metrics.
///
/// Jobs are identified by their `jobid`; multiple processes (steps) belonging
/// to the same job are counted once.
#[derive(Debug, Default)]
pub struct DefaultProfiler;

impl DefaultProfiler {
    pub fn new() -> Self {
        Self
    }
}

impl Profiler for DefaultProfiler {
    /// Count distinct job IDs and emit a single `kys_running_jobs` metric.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes reported by the HPC scheduler.
    ///
    /// # Returns
    ///
    /// A single-element vector containing the `kys_running_jobs` metric.
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>> {
        let unique_jobs: HashSet<&str> = processes.iter().map(|p| p.jobid.as_str()).collect();

        Ok(vec![Metric {
            name: "kys_running_jobs",
            labels: vec![("hostname", HOSTNAME.clone())],
            value: unique_jobs.len() as f64,
        }])
    }
}
