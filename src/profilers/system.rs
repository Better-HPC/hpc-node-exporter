//! Combined node-level and job-level system profiler.
//!
//! This module provides [`SystemProfiler`], which uses the [`sysinfo`] crate to
//! collect telemetry for CPU, memory, and network utilization.

use std::collections::HashMap;
use std::error::Error;

use sysinfo::{Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// Aggregated resource usage for a single job step.
///
/// Represents system usage summed over all active process running under the same `(jobid, stepid)`.
#[derive(Debug, Default)]
struct JobSnapshot {
    cpu_usage: f32,
    memory_bytes: u64,
    io_read_bytes: u64,
    io_written_bytes: u64,
}

/// A [`Profiler`] that collects node and job-level system metrics.
///
/// Internally wraps a persistent [`sysinfo::System`] instance (for CPU, memory,
/// and process queries) and a [`sysinfo::Networks`] instance (for network I/O).
/// Both are long-lived so that `sysinfo` can compute meaningful deltas between
/// consecutive scrapes.
#[derive(Debug)]
pub struct SystemProfiler {
    sys: System,
    networks: Networks,
}

impl SystemProfiler {
    /// Create a new profiler with pre-warmed CPU and network baselines.
    ///
    /// The `sysinfo` crate reports CPU usage as a delta between successive
    /// [`System::refresh_cpu_usage`] calls. This constructor performs the
    /// first refresh so successive calls return a real value instead of a
    /// meaningless zero placeholder.
    ///
    /// Similarly, [`Networks::new_with_refreshed_list`] snapshots the
    /// current set of network interfaces so later refreshes can track
    /// per-interval byte counters.
    ///
    /// # Returns
    ///
    /// A new [`SystemProfiler`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the host operating system is not supported by
    /// the `sysinfo` crate.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err("SystemProfiler: OS not supported by sysinfo".into());
        }

        // Perform a baseline measurement for system CPU/network metrics
        let networks = Networks::new_with_refreshed_list();
        let mut sys = System::new();
        sys.refresh_cpu_usage();

        Ok(Self { sys, networks })
    }

    /// Collect CPU metrics summed across all cores.
    ///
    /// Calls [`System::refresh_cpu_usage`] to capture a new sample, then
    /// sums the per-core values from [`System::cpus`]. 100% utilization
    /// represents full utilization of one core, so a 4-core machine at
    /// full load reports 400%.
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics including: `node_cpu_usage_percent`.
    fn collect_cpu(&mut self) -> Vec<Metric> {
        self.sys.refresh_cpu_usage();
        let total_cpu: f64 = self.sys.cpus().iter().map(
            |c| c.cpu_usage() as f64
        ).sum();

        vec![Metric {
            name: "node_cpu_usage_percent",
            labels: vec![],
            value: total_cpu,
        }]
    }

    /// Collect physical memory metrics.
    ///
    /// Returns three metrics representing total installed memory, memory
    /// currently in use, and memory available for new allocations. Values
    /// come from [`System::refresh_memory`] and reflect the kernel's view
    /// at the time of the call.
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics including:
    /// `node_memory_total_bytes`, `node_memory_used_bytes`, and `node_memory_available_bytes`.
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

    /// Collect network throughput metrics.
    ///
    /// Iterates over all known network interfaces, skipping the loopback
    /// device (`lo`), and sums received and transmitted bytes into two
    /// aggregate counters. The values represent bytes transferred since the
    /// previous call to [`Networks::refresh`], making them suitable for
    /// rate calculations in Prometheus (e.g., `rate(node_net_rx_bytes[5m])`).
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics including:
    /// `node_net_rx_bytes` and `node_net_tx_bytes`.
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

    /// Build per-job resource snapshots by aggregating current process data.
    ///
    /// For each [`HpcProcess`] in the input slice, this method:
    ///
    /// 1. Converts the PID to a [`sysinfo::Pid`] and asks `sysinfo` to
    ///    refresh only the CPU, memory, and disk-usage fields for that set
    ///    of processes (avoiding a full process-table scan).
    /// 2. Looks the process up in the refreshed table. If the PID is no
    ///    longer running (e.g., it exited between the scheduler query and
    ///    now), a warning is printed and the process is skipped.
    /// 3. Accumulates the process's stats into the [`JobSnapshot`] keyed
    ///    by `(jobid, stepid)`.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A map from `(jobid, stepid)` to the aggregated [`JobSnapshot`] for that job step.
    fn collect_job_snapshots(
        &mut self,
        processes: &[HpcProcess],
    ) -> HashMap<(String, String), JobSnapshot> {
        let pids: Vec<Pid> = processes
            .iter()
            .map(|p| Pid::from(p.pid as usize))
            .collect();

        let refresh_kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage();

        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::Some(&pids), false, refresh_kind);

        let mut jobs: HashMap<(String, String), JobSnapshot> = HashMap::new();
        for proc in processes {
            let pid = Pid::from(proc.pid as usize);
            let Some(info) = self.sys.process(pid) else {
                eprintln!("warning: pid {} not found", proc.pid);
                continue;
            };

            let snap = jobs
                .entry((proc.jobid.clone(), proc.stepid.clone()))
                .or_default();

            snap.cpu_usage += info.cpu_usage();
            snap.memory_bytes += info.memory();

            let disk = info.disk_usage();
            snap.io_read_bytes += disk.read_bytes;
            snap.io_written_bytes += disk.written_bytes;
        }

        jobs
    }

    /// Collect per-job resource usage.
    ///
    /// Delegates to [`collect_job_snapshots`](Self::collect_job_snapshots)
    /// to build per-job usage data, then flattens each snapshot into four
    /// metrics (CPU, memory, I/O read, I/O write), each labeled with the
    /// originating `jobid` and `stepid`.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics including:
    /// `job_cpu_usage_percent`, `job_memory_used_bytes`, `job_io_read_bytes`,
    /// and `job_io_write_bytes`.
    fn collect_job_metrics(&mut self, processes: &[HpcProcess]) -> Vec<Metric> {
        let snapshots = self.collect_job_snapshots(processes);
        let mut metrics = Vec::new();

        for ((jobid, stepid), snap) in &snapshots {
            let labels = vec![("jobid", jobid.clone()), ("stepid", stepid.clone())];

            metrics.push(Metric {
                name: "job_cpu_usage_percent",
                labels: labels.clone(),
                value: snap.cpu_usage as f64,
            });

            metrics.push(Metric {
                name: "job_memory_used_bytes",
                labels: labels.clone(),
                value: snap.memory_bytes as f64,
            });

            metrics.push(Metric {
                name: "job_io_read_bytes",
                labels: labels.clone(),
                value: snap.io_read_bytes as f64,
            });

            metrics.push(Metric {
                name: "job_io_write_bytes",
                labels,
                value: snap.io_written_bytes as f64,
            });
        }

        metrics
    }
}

impl Profiler for SystemProfiler {
    /// Collect all node and job-level system metrics.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics.
    ///
    /// # Errors
    ///
    /// Returns an error if a fundamental `sysinfo` failure occurs.
    /// Individual process query failures are logged as warnings and
    /// skipped rather than propagated.
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>> {
        let mut metrics = Vec::new();
        metrics.extend(self.collect_cpu());
        metrics.extend(self.collect_memory());
        metrics.extend(self.collect_network());
        metrics.extend(self.collect_job_metrics(processes));
        Ok(metrics)
    }
}
