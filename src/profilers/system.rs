//! Combined node-level and job-level system profiler.
//!
//! This module provides [`SystemProfiler`], which uses the [`sysinfo`] crate
//! to collect telemetry for CPU, memory, swap, and per-job resource utilization.

use std::collections::HashMap;
use std::error::Error;

use log::warn;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::profilers::{Metric, Profiler, HOSTNAME};
use crate::schedulers::HpcProcess;

/// Aggregated resource usage for a single job step.
///
/// Represents system usage summed over all active HPC processes running
/// under the same `(jobid, stepid)`.
#[derive(Debug, Default)]
struct SystemJobSnapshot {
    cpu_usage: f32,
    memory_bytes: u64,
    virtual_memory_bytes: u64,
    io_read_bytes: u64,
    io_written_bytes: u64,
    process_count: u32,
}

/// A [`Profiler`] that collects node and job-level system metrics.
///
/// Internally wraps a persistent [`sysinfo::System`] instance for CPU, memory,
/// and process queries. The instance is long-lived so that `sysinfo` can
/// compute meaningful deltas between consecutive scrapes.
#[derive(Debug)]
pub struct SystemProfiler {
    sys: System,
}

impl SystemProfiler {
    /// Create a new profiler with a pre-warmed CPU baseline.
    ///
    /// System info is generally reported as a delta between successive
    /// measurements. This constructor performs an initial measurement
    /// so successive calls return a meaningful value.
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

        let mut sys = System::new();
        sys.refresh_cpu_usage();

        Ok(Self { sys })
    }

    /// Return common labels for node-level metrics.
    fn node_labels() -> Vec<(&'static str, String)> {
        vec![("hostname", HOSTNAME.clone())]
    }

    /// Return common labels for core-level metrics.
    fn core_labels(core_id: usize) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("core", core_id.to_string()),
        ]
    }

    /// Return common labels for a job-level metric.
    fn job_labels(jobid: &str, stepid: &str) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("jobid", jobid.to_string()),
            ("stepid", stepid.to_string()),
        ]
    }

    /// Collect CPU metrics across all cores.
    ///
    /// # Returns
    ///
    /// A vector of node-level CPU metrics:
    ///
    /// * `kys_sys_cpu_usage_percent`
    /// * `kys_sys_cpu_count`
    /// * `kys_sys_cpu_core_usage_percent`
    /// * `kys_sys_load_avg_1m`
    /// * `kys_sys_load_avg_5m`
    /// * `kys_sys_load_avg_15m`
    fn collect_cpu(&mut self) -> Vec<Metric> {
        self.sys.refresh_cpu_usage();

        let cpus = self.sys.cpus();
        let total_cpu: f64 = cpus.iter().map(|c| c.cpu_usage() as f64).sum();
        let cpu_count = cpus.len() as f64;
        let load = System::load_average();

        let mut metrics = vec![
            Metric {
                name: "kys_sys_cpu_usage_percent",
                labels: Self::node_labels(),
                value: total_cpu,
            },
            Metric {
                name: "kys_sys_cpu_count",
                labels: Self::node_labels(),
                value: cpu_count,
            },
            Metric {
                name: "kys_sys_load_avg_1m",
                labels: Self::node_labels(),
                value: load.one,
            },
            Metric {
                name: "kys_sys_load_avg_5m",
                labels: Self::node_labels(),
                value: load.five,
            },
            Metric {
                name: "kys_sys_load_avg_15m",
                labels: Self::node_labels(),
                value: load.fifteen,
            },
        ];

        // Per-core utilization
        for (i, cpu) in cpus.iter().enumerate() {
            metrics.push(Metric {
                name: "kys_sys_cpu_core_usage_percent",
                labels: Self::core_labels(i),
                value: cpu.cpu_usage() as f64,
            });
        }

        metrics
    }

    /// Collect physical memory and swap metrics.
    ///
    /// # Returns
    ///
    /// A vector of node-level memory and swap metrics:
    ///
    /// * `kys_sys_memory_total_bytes`
    /// * `kys_sys_memory_used_bytes`
    /// * `kys_sys_memory_available_bytes`
    /// * `kys_sys_swap_total_bytes`
    /// * `kys_sys_swap_used_bytes`
    /// * `kys_sys_swap_free_bytes`
    fn collect_memory(&mut self) -> Vec<Metric> {
        self.sys.refresh_memory();

        vec![
            Metric {
                name: "kys_sys_memory_total_bytes",
                labels: Self::node_labels(),
                value: self.sys.total_memory() as f64,
            },
            Metric {
                name: "kys_sys_memory_used_bytes",
                labels: Self::node_labels(),
                value: self.sys.used_memory() as f64,
            },
            Metric {
                name: "kys_sys_memory_available_bytes",
                labels: Self::node_labels(),
                value: self.sys.available_memory() as f64,
            },
            Metric {
                name: "kys_sys_swap_total_bytes",
                labels: Self::node_labels(),
                value: self.sys.total_swap() as f64,
            },
            Metric {
                name: "kys_sys_swap_used_bytes",
                labels: Self::node_labels(),
                value: self.sys.used_swap() as f64,
            },
            Metric {
                name: "kys_sys_swap_free_bytes",
                labels: Self::node_labels(),
                value: self.sys.free_swap() as f64,
            },
        ]
    }

    /// Build per-job resource snapshots by aggregating current process data.
    ///
    /// For each [`HpcProcess`] in the input slice, this method:
    ///
    /// 1. Converts the PID to a [`sysinfo::Pid`] and asks `sysinfo` to
    ///    refresh CPU, memory, and disk-usage fields for that set of
    ///    processes (avoiding a full process-table scan).
    /// 2. Looks the process up in the refreshed table. If the PID is no
    ///    longer running (e.g., it exited between the scheduler query and
    ///    now), a warning is printed and the process is skipped.
    /// 3. Accumulates the process's stats into the [`SystemJobSnapshot`]
    ///    keyed by `(jobid, stepid)`.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A map from `(jobid, stepid)` to the aggregated [`SystemJobSnapshot`].
    fn collect_job_snapshots(
        &mut self,
        processes: &[HpcProcess],
    ) -> HashMap<(String, String), SystemJobSnapshot> {
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

        let mut jobs: HashMap<(String, String), SystemJobSnapshot> = HashMap::new();
        for proc in processes {
            let pid = Pid::from(proc.pid as usize);
            let Some(info) = self.sys.process(pid) else {
                warn!("pid {} not found (job {}, step {})", proc.pid, proc.jobid, proc.stepid);
                continue;
            };

            let snap = jobs
                .entry((proc.jobid.clone(), proc.stepid.clone()))
                .or_default();

            snap.cpu_usage += info.cpu_usage();
            snap.memory_bytes += info.memory();
            snap.virtual_memory_bytes += info.virtual_memory();

            let disk = info.disk_usage();
            snap.io_read_bytes += disk.read_bytes;
            snap.io_written_bytes += disk.written_bytes;

            snap.process_count += 1;
        }

        jobs
    }

    /// Collect per-job resource usage metrics.
    ///
    /// Delegates to [`collect_job_snapshots`](Self::collect_job_snapshots)
    /// to build per-job usage data, then flattens each snapshot into
    /// metrics labeled with the originating `jobid` and `stepid`.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A vector of per-job metrics, including:
    ///
    /// * `kys_sys_job_cpu_usage_percent`
    /// * `kys_sys_job_memory_used_bytes`
    /// * `kys_sys_job_virtual_memory_bytes`
    /// * `kys_sys_job_io_read_bytes`
    /// * `kys_sys_job_io_write_bytes`
    /// * `kys_sys_job_process_count`
    fn collect_job_metrics(&mut self, processes: &[HpcProcess]) -> Vec<Metric> {
        let snapshots = self.collect_job_snapshots(processes);
        let mut metrics = Vec::new();

        for ((jobid, stepid), snap) in &snapshots {
            let labels = Self::job_labels(jobid, stepid);

            metrics.push(Metric {
                name: "kys_sys_job_cpu_usage_percent",
                labels: labels.clone(),
                value: snap.cpu_usage as f64,
            });

            metrics.push(Metric {
                name: "kys_sys_job_memory_used_bytes",
                labels: labels.clone(),
                value: snap.memory_bytes as f64,
            });

            metrics.push(Metric {
                name: "kys_sys_job_virtual_memory_bytes",
                labels: labels.clone(),
                value: snap.virtual_memory_bytes as f64,
            });

            metrics.push(Metric {
                name: "kys_sys_job_io_read_bytes",
                labels: labels.clone(),
                value: snap.io_read_bytes as f64,
            });

            metrics.push(Metric {
                name: "kys_sys_job_io_write_bytes",
                labels: labels.clone(),
                value: snap.io_written_bytes as f64,
            });

            metrics.push(Metric {
                name: "kys_sys_job_process_count",
                labels,
                value: snap.process_count as f64,
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
        metrics.extend(self.collect_job_metrics(processes));
        Ok(metrics)
    }
}
