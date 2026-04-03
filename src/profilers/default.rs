//! Default profiler that is always enabled.
//!
//! Reports scheduler-level metrics derived entirely from the HPC process
//! list, such as the number of running jobs and the current scrape
//! timestamp.

use std::collections::HashSet;
use std::error::Error;
use std::time::SystemTime;

use crate::metrics::{MetricFamily, MetricType};
use crate::profilers::{Profiler, HOSTNAME};
use crate::schedulers::HpcProcess;

/// A [`Profiler`] that reports baseline HPC scheduler metrics.
///
/// Jobs are identified by their `jobid`; multiple processes (steps)
/// belonging to the same job are counted once.
#[derive(Debug, Default)]
pub struct DefaultProfiler;

impl DefaultProfiler {
    /// Creates a new `DefaultProfiler`.
    pub fn new() -> Self {
        Self
    }
}

impl Profiler for DefaultProfiler {
    /// Returns high level status metrics.
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<MetricFamily>, Box<dyn Error>> {
        let labels = vec![("hostname", HOSTNAME.clone())];

        let mut running_jobs = MetricFamily::new(
            "hpcexp_running_jobs",
            "Number of HPC jobs currently running on the node.",
            MetricType::Gauge,
        );

        let unique_jobs: HashSet<&str> = processes.iter().map(|p| p.jobid.as_str()).collect();
        running_jobs.add(labels.clone(), unique_jobs.len() as f64);

        let mut scrape_time = MetricFamily::new(
            "hpcexp_scrape_time",
            "Unix timestamp of the last metrics collection pass.",
            MetricType::Gauge,
        );

        let epoch_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        scrape_time.add(labels, epoch_time.as_secs_f64());

        Ok(vec![running_jobs, scrape_time])
    }
}
