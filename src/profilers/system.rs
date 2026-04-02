//! Hardware profiler for common system metrics (CPU, memory, etc.).
//!
//! Relies on the [`sysinfo`] crate to collect system resource utilization.

use std::collections::HashMap;
use std::error::Error;

use log::warn;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::metrics::{MetricFamily, MetricSample, MetricType};
use crate::profilers::{Profiler, HOSTNAME};
use crate::schedulers::HpcProcess;

/// Aggregated per-job system usage across one or more processes.
#[derive(Debug, Default)]
struct SystemJobSnapshot {
    cpu_usage: f32,
    memory_bytes: u64,
    virtual_memory_bytes: u64,
    io_read_bytes: u64,
    io_written_bytes: u64,
    process_count: u32,
}

/// A [`Profiler`] for common system metrics (CPU, memory, swap).
#[derive(Debug)]
pub struct SystemProfiler {
    sys: System,
}

impl SystemProfiler {
    /// Initialize hardware measurements and return a new profiler.
    ///
    /// # Errors
    ///
    /// Returns an error if the host OS is not supported by `sysinfo`.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err("SystemProfiler: OS not supported by sysinfo".into());
        }

        let mut sys = System::new();
        sys.refresh_cpu_usage();

        Ok(Self { sys })
    }

    /// Returns common labels for node-level metrics.
    fn node_labels() -> Vec<(&'static str, String)> {
        vec![("hostname", HOSTNAME.clone())]
    }

    /// Returns common labels for per-core metrics.
    fn core_labels(core_id: usize) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("core", core_id.to_string()),
        ]
    }

    /// Returns common labels for job-level metrics.
    fn job_labels(jobid: &str, stepid: &str) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("jobid", jobid.to_string()),
            ("stepid", stepid.to_string()),
        ]
    }

    /// Builds per-job resource snapshots by aggregating process data.
    ///
    /// For each process in `processes`, refreshes its CPU, memory, and disk
    /// usage via `sysinfo` and accumulates the results into snapshots keyed
    /// by `(jobid, stepid)`. Processes that have exited since the scheduler
    /// query are logged and skipped.
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
                warn!(
                    "pid {} reported by scheduler but not not found by profiler (job {}, step {})",
                    proc.pid, proc.jobid, proc.stepid
                );
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

    /// Collects CPU metrics across all cores.
    fn collect_cpu(&mut self) -> Vec<MetricFamily> {
        self.sys.refresh_cpu_usage();

        let cpus = self.sys.cpus();
        let cpu_count = cpus.len() as f64;
        let total_cpu: f64 = cpus.iter().map(|c| c.cpu_usage() as f64).sum();

        let load = System::load_average();
        let labels = Self::node_labels();

        let mut families = vec![
            MetricFamily::from_samples(
                "hpcexp_sys_cpu_usage_percent",
                "Total CPU usage across all cores as a percentage.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: total_cpu,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_cpu_count",
                "Number of logical CPU cores available on this node.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: cpu_count,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_load_avg_1m",
                "System load average over the last 1 minute.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: load.one,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_load_avg_5m",
                "System load average over the last 5 minutes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: load.five,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_load_avg_15m",
                "System load average over the last 15 minutes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels,
                    value: load.fifteen,
                }],
            ),
        ];

        // Per-core utilization: all cores share one family, one sample each.
        let mut core_usage = MetricFamily::new(
            "hpcexp_sys_cpu_core_usage_percent",
            "CPU usage per logical core as a percentage.",
            MetricType::Gauge,
        );

        for (i, cpu) in cpus.iter().enumerate() {
            core_usage.add(Self::core_labels(i), cpu.cpu_usage() as f64);
        }

        families.push(core_usage);
        families
    }

    /// Returns physical memory and swap metrics.
    fn collect_memory(&mut self) -> Vec<MetricFamily> {
        self.sys.refresh_memory();
        let labels = Self::node_labels();

        vec![
            MetricFamily::from_samples(
                "hpcexp_sys_memory_total_bytes",
                "Total physical memory available on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.total_memory() as f64,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_memory_used_bytes",
                "Physical memory currently in use on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.used_memory() as f64,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_memory_available_bytes",
                "Physical memory currently available on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.available_memory() as f64,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_swap_total_bytes",
                "Total swap space on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.total_swap() as f64,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_swap_used_bytes",
                "Swap space currently in use on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.used_swap() as f64,
                }],
            ),
            MetricFamily::from_samples(
                "hpcexp_sys_swap_free_bytes",
                "Swap space currently free on this node in bytes.",
                MetricType::Gauge,
                vec![MetricSample {
                    labels: labels.clone(),
                    value: self.sys.free_swap() as f64,
                }],
            ),
        ]
    }

    /// Returns per-job resource usage metrics.
    ///
    /// Delegates to [`collect_job_snapshots`](Self::collect_job_snapshots)
    /// and flattens each snapshot into labeled metric families.
    fn collect_job_metrics(&mut self, processes: &[HpcProcess]) -> Vec<MetricFamily> {
        let snapshots = self.collect_job_snapshots(processes);
        if snapshots.is_empty() {
            return Vec::new();
        }

        let mut cpu = MetricFamily::new(
            "hpcexp_sys_job_cpu_usage_percent",
            "Total CPU usage for an HPC job step across all its processes, as a percentage.",
            MetricType::Gauge,
        );

        let mut mem = MetricFamily::new(
            "hpcexp_sys_job_memory_used_bytes",
            "Physical memory used by an HPC job step across all its processes, in bytes.",
            MetricType::Gauge,
        );

        let mut vmem = MetricFamily::new(
            "hpcexp_sys_job_virtual_memory_bytes",
            "Virtual memory used by an HPC job step across all its processes, in bytes.",
            MetricType::Gauge,
        );

        let mut io_read = MetricFamily::new(
            "hpcexp_sys_job_io_read_bytes",
            "Bytes read from disk by an HPC job step since it started.",
            MetricType::Counter,
        );

        let mut io_write = MetricFamily::new(
            "hpcexp_sys_job_io_write_bytes",
            "Bytes written to disk by an HPC job step since it started.",
            MetricType::Counter,
        );

        let mut procs = MetricFamily::new(
            "hpcexp_sys_job_process_count",
            "Number of running processes belonging to an HPC job step.",
            MetricType::Gauge,
        );

        for ((jobid, stepid), snap) in &snapshots {
            let labels = Self::job_labels(jobid, stepid);
            cpu.add(labels.clone(), snap.cpu_usage as f64);
            mem.add(labels.clone(), snap.memory_bytes as f64);
            vmem.add(labels.clone(), snap.virtual_memory_bytes as f64);
            io_read.add(labels.clone(), snap.io_read_bytes as f64);
            io_write.add(labels.clone(), snap.io_written_bytes as f64);
            procs.add(labels, snap.process_count as f64);
        }

        vec![cpu, mem, vmem, io_read, io_write, procs]
    }
}

impl Profiler for SystemProfiler {
    /// Measures and returns all system usage metrics.
    ///
    /// Individual process query failures are logged and skipped rather
    /// than propagated.
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<MetricFamily>, Box<dyn Error>> {
        let mut families = Vec::new();
        families.extend(self.collect_cpu());
        families.extend(self.collect_memory());
        families.extend(self.collect_job_metrics(processes));
        Ok(families)
    }
}
