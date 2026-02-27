//! Node-level system profiler.
//!
//! Collects machine-wide telemetry using the `sysinfo` crate. Metrics:
//!
//! - CPU utilization (aggregate usage percentage)
//! - Memory usage (total, used, available bytes)
//! - Network I/O (rx/tx bytes per scrape interval, excluding loopback)

use std::error::Error;

use sysinfo::{Networks, System};

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// A [`Profiler`] for collecting node-level system metrics.
#[derive(Debug)]
pub struct SysNodeProfiler {
    sys: System,
    networks: Networks,
}

impl Default for SysNodeProfiler {
    fn default() -> Self {
        let networks = Networks::new_with_refreshed_list();
        let mut sys = System::new();

        // CPU measurements are calculated as a delta.
        // Perform the first measurement here so later calls are accurate.
        sys.refresh_cpu_usage();
        Self { sys, networks }
    }
}

impl SysNodeProfiler {
    /// Collect aggregate CPU usage as a single percentage.
    fn collect_cpu(&mut self) -> Vec<Metric> {
        self.sys.refresh_cpu_usage();

        vec![Metric {
            name: "node_cpu_usage_percent",
            labels: vec![],
            value: self.sys.global_cpu_usage() as f64,
        }]
    }

    /// Collect memory metrics (total, used, available) in bytes.
    fn collect_memory(&mut self) -> Vec<Metric> {
        self.sys.refresh_memory();

        vec![
            Metric {
                name: "node_memory_total_bytes",
                labels: vec![],
                value: self.sys.total_memory() as f64,
            },
            Metric {
                name: "node_memory_used_bytes",
                labels: vec![],
                value: self.sys.used_memory() as f64,
            },
            Metric {
                name: "node_memory_available_bytes",
                labels: vec![],
                value: self.sys.available_memory() as f64,
            },
        ]
    }

    /// Collect network metrics (rx, tx) in bytes across all interfaces except loopback (`lo`).
    fn collect_network(&mut self) -> Vec<Metric> {
        self.networks.refresh(false);

        let mut total_rx: u64 = 0;
        let mut total_tx: u64 = 0;

        for (iface, data) in &self.networks {
            if iface == "lo" {
                continue;
            }
            total_rx += data.received();
            total_tx += data.transmitted();
        }

        vec![
            Metric {
                name: "node_net_rx_bytes",
                labels: vec![],
                value: total_rx as f64,
            },
            Metric {
                name: "node_net_tx_bytes",
                labels: vec![],
                value: total_tx as f64,
            },
        ]
    }
}

impl Profiler for SysNodeProfiler {
    fn is_supported(&self) -> Result<(), String> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err("SysNodeProfiler: OS not supported by sysinfo".to_string());
        }
        Ok(())
    }

    /// Collect node-level system metrics.
    ///
    /// The `processes` argument is ignored since node-level metrics are
    /// not attributed to individual jobs.
    fn collect_metrics(
        &mut self,
        _processes: &[HpcProcess],
    ) -> Result<Vec<Metric>, Box<dyn Error>> {
        let mut metrics = Vec::new();
        metrics.extend(self.collect_cpu());
        metrics.extend(self.collect_memory());
        metrics.extend(self.collect_network());
        Ok(metrics)
    }
}
