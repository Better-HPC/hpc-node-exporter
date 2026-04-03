//! Hardware profiler for common system metrics (CPU, memory, etc.).
//!
//! Relies on the [`sysinfo`] crate to collect system resource utilization.

use std::collections::HashMap;
use std::error::Error;

use log::warn;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::metrics::{MetricFamily, MetricType};
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

        // System usage is calculated as a delta between measurements.
        // Preload the first measurement at init so latter measurements are valid.
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

        self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), false, refresh_kind);

        let mut jobs: HashMap<(String, String), SystemJobSnapshot> = HashMap::new();
        for proc in processes {
            let pid = Pid::from(proc.pid as usize);
            let Some(info) = self.sys.process(pid) else {
                warn!(
                    "pid {} reported by scheduler but not found by profiler (job {}, step {})",
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
        let mut cpu_usage_metrics = MetricFamily::new(
            "hpcexp_sys_cpu_usage_percent",
            "Total CPU usage across all cores as a percentage.",
            MetricType::Gauge,
        );

        let mut cpu_count_metrics = MetricFamily::new(
            "hpcexp_sys_cpu_count",
            "Number of logical CPU cores available on this node.",
            MetricType::Gauge,
        );

        let mut load_1m_metrics = MetricFamily::new(
            "hpcexp_sys_load_avg_1m",
            "System load average over the last 1 minute.",
            MetricType::Gauge,
        );

        let mut load_5m_metrics = MetricFamily::new(
            "hpcexp_sys_load_avg_5m",
            "System load average over the last 5 minutes.",
            MetricType::Gauge,
        );

        let mut load_15m_metrics = MetricFamily::new(
            "hpcexp_sys_load_avg_15m",
            "System load average over the last 15 minutes.",
            MetricType::Gauge,
        );

        let mut core_usage_metrics = MetricFamily::new(
            "hpcexp_sys_cpu_core_usage_percent",
            "CPU usage per logical core as a percentage.",
            MetricType::Gauge,
        );

        let labels = Self::node_labels();
        self.sys.refresh_cpu_usage();

        let cpus = self.sys.cpus();
        let load = System::load_average();
        let total_cpu = cpus.iter().map(|c| c.cpu_usage() as f64).sum();

        cpu_usage_metrics.add(labels.clone(), total_cpu);
        cpu_count_metrics.add(labels.clone(), cpus.len() as f64);
        load_1m_metrics.add(labels.clone(), load.one);
        load_5m_metrics.add(labels.clone(), load.five);
        load_15m_metrics.add(labels, load.fifteen);

        for (i, cpu) in cpus.iter().enumerate() {
            core_usage_metrics.add(Self::core_labels(i), cpu.cpu_usage() as f64);
        }

        vec![
            cpu_usage_metrics,
            cpu_count_metrics,
            load_1m_metrics,
            load_5m_metrics,
            load_15m_metrics,
            core_usage_metrics,
        ]
    }

    /// Returns physical memory and swap metrics.
    fn collect_memory(&mut self) -> Vec<MetricFamily> {
        let mut mem_total_metrics = MetricFamily::new(
            "hpcexp_sys_memory_total_bytes",
            "Total physical memory available on this node in bytes.",
            MetricType::Gauge,
        );

        let mut mem_used_metrics = MetricFamily::new(
            "hpcexp_sys_memory_used_bytes",
            "Physical memory currently in use on this node in bytes.",
            MetricType::Gauge,
        );

        let mut mem_avail_metrics = MetricFamily::new(
            "hpcexp_sys_memory_available_bytes",
            "Physical memory currently available on this node in bytes.",
            MetricType::Gauge,
        );

        let mut swap_total_metrics = MetricFamily::new(
            "hpcexp_sys_swap_total_bytes",
            "Total swap space on this node in bytes.",
            MetricType::Gauge,
        );

        let mut swap_used_metrics = MetricFamily::new(
            "hpcexp_sys_swap_used_bytes",
            "Swap space currently in use on this node in bytes.",
            MetricType::Gauge,
        );

        let mut swap_free_metrics = MetricFamily::new(
            "hpcexp_sys_swap_free_bytes",
            "Swap space currently free on this node in bytes.",
            MetricType::Gauge,
        );

        let labels = Self::node_labels();
        self.sys.refresh_memory();

        mem_total_metrics.add(labels.clone(), self.sys.total_memory() as f64);
        mem_used_metrics.add(labels.clone(), self.sys.used_memory() as f64);
        mem_avail_metrics.add(labels.clone(), self.sys.available_memory() as f64);
        swap_total_metrics.add(labels.clone(), self.sys.total_swap() as f64);
        swap_used_metrics.add(labels.clone(), self.sys.used_swap() as f64);
        swap_free_metrics.add(labels, self.sys.free_swap() as f64);

        vec![
            mem_total_metrics,
            mem_used_metrics,
            mem_avail_metrics,
            swap_total_metrics,
            swap_used_metrics,
            swap_free_metrics,
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

        let mut cpu_metrics = MetricFamily::new(
            "hpcexp_sys_job_cpu_usage_percent",
            "Total CPU usage for an HPC job step across all its processes, as a percentage.",
            MetricType::Gauge,
        );

        let mut mem_metrics = MetricFamily::new(
            "hpcexp_sys_job_memory_used_bytes",
            "Physical memory used by an HPC job step across all its processes, in bytes.",
            MetricType::Gauge,
        );

        let mut vmem_metrics = MetricFamily::new(
            "hpcexp_sys_job_virtual_memory_bytes",
            "Virtual memory used by an HPC job step across all its processes, in bytes.",
            MetricType::Gauge,
        );

        let mut io_read_metrics = MetricFamily::new(
            "hpcexp_sys_job_io_read_bytes",
            "Bytes read from disk by an HPC job step since it started.",
            MetricType::Counter,
        );

        let mut io_write_metrics = MetricFamily::new(
            "hpcexp_sys_job_io_write_bytes",
            "Bytes written to disk by an HPC job step since it started.",
            MetricType::Counter,
        );

        let mut procs_metrics = MetricFamily::new(
            "hpcexp_sys_job_process_count",
            "Number of running processes belonging to an HPC job step.",
            MetricType::Gauge,
        );

        for ((jobid, stepid), snap) in &snapshots {
            let labels = Self::job_labels(jobid, stepid);
            cpu_metrics.add(labels.clone(), snap.cpu_usage as f64);
            mem_metrics.add(labels.clone(), snap.memory_bytes as f64);
            vmem_metrics.add(labels.clone(), snap.virtual_memory_bytes as f64);
            io_read_metrics.add(labels.clone(), snap.io_read_bytes as f64);
            io_write_metrics.add(labels.clone(), snap.io_written_bytes as f64);
            procs_metrics.add(labels, snap.process_count as f64);
        }

        vec![
            cpu_metrics,
            mem_metrics,
            vmem_metrics,
            io_read_metrics,
            io_write_metrics,
            procs_metrics,
        ]
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
