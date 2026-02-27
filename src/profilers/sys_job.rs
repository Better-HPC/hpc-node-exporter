//! Job-level system profiler.
//!
//! Collects per-job telemetry using the `sysinfo` crate by looking up each
//! PID associated with an HPC job. Metrics are aggregated by (jobid, stepid):
//!
//! - CPU utilization (total usage percent, per-core style)
//! - Memory usage (RSS in bytes)
//! - I/O bytes read/written (from `disk_usage()`)
//!
//! CPU percent is computed by `sysinfo` as a delta from the previous refresh,
//! so the first scrape will report 0% for all jobs.

use std::collections::HashMap;
use std::error::Error;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// A [`Profiler`] that collects job-level system metrics via `sysinfo`.
#[derive(Debug)]
pub struct SysJobProfiler {
    sys: System,
}

impl Default for SysJobProfiler {
    fn default() -> Self {
        Self { sys: System::new() }
    }
}

/// Aggregated per-job snapshot built from individual process data.
#[derive(Debug, Default)]
struct JobSnapshot {
    cpu_usage: f32,
    memory_bytes: u64,
    io_read_bytes: u64,
    io_written_bytes: u64,
}

impl SysJobProfiler {
    /// Refresh the specific PIDs from the process list and aggregate
    /// their metrics by (jobid, stepid).
    fn collect_snapshots(
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
                eprintln!("warning: pid {} not found in sysinfo", proc.pid);
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
}

impl Profiler for SysJobProfiler {
    fn is_supported(&self) -> Result<(), String> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err("SysJobProfiler: OS not supported by sysinfo".to_string());
        }
        Ok(())
    }

    /// Collect job-level system metrics by refreshing per-PID process data
    /// and aggregating by (jobid, stepid).
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>> {
        let snapshots = self.collect_snapshots(processes);
        let mut metrics = Vec::new();

        for ((jobid, stepid), snap) in &snapshots {
            let jid = Some(jobid.clone());
            let sid = Some(stepid.clone());

            metrics.push(Metric {
                name: "job_cpu_usage_percent",
                jobid: jid.clone(),
                stepid: sid.clone(),
                value: snap.cpu_usage as f64,
            });

            metrics.push(Metric {
                name: "job_memory_used_bytes",
                jobid: jid.clone(),
                stepid: sid.clone(),
                value: snap.memory_bytes as f64,
            });

            metrics.push(Metric {
                name: "job_io_read_bytes",
                jobid: jid.clone(),
                stepid: sid.clone(),
                value: snap.io_read_bytes as f64,
            });

            metrics.push(Metric {
                name: "job_io_write_bytes",
                jobid: jid,
                stepid: sid,
                value: snap.io_written_bytes as f64,
            });
        }

        Ok(metrics)
    }
}
