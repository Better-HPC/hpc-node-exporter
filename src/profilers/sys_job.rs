//! Job-level system profiler.
//!
//! Collects per-job telemetry by reading `/proc/[pid]/stat` and
//! `/proc/[pid]/io` for every PID associated with an HPC job, then
//! aggregating the results by job/step. Metrics include:
//!
//! - CPU utilization (user and system percent, delta between scrapes)
//! - Memory usage (RSS in bytes)
//! - Thread count
//! - I/O bytes read/written (from `/proc/[pid]/io`, requires privileges)
//!
//! CPU percent is computed per-core style: a job using 4 cores fully
//! will report 400%. On the first scrape, CPU percent metrics are
//! omitted since there is no previous sample to compute a delta from.

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::Instant;

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// A [`Profiler`] that collects job-level system metrics from `/proc/[pid]`.
#[derive(Debug)]
pub struct SysJobProfiler {
    /// Previous CPU samples and wall-clock timestamp, keyed by (jobid, stepid).
    prev_samples: Option<(Instant, HashMap<(String, String), CpuSample>)>,
}

impl Default for SysJobProfiler {
    fn default() -> Self {
        Self { prev_samples: None }
    }
}

/// Aggregated CPU jiffies for a single job step.
#[derive(Debug, Clone, Default)]
struct CpuSample {
    utime: u64,
    stime: u64,
}

/// Snapshot of per-job metrics collected in a single pass over all PIDs.
#[derive(Debug, Default)]
struct JobSnapshot {
    cpu: CpuSample,
    rss_pages: u64,
    num_threads: u64,
    io_read_bytes: Option<u64>,
    io_write_bytes: Option<u64>,
}

impl SysJobProfiler {
    /// Parse `/proc/[pid]/stat` and accumulate values into the job snapshot.
    ///
    /// Fields of interest (1-indexed per `proc(5)`):
    ///   - 14: utime (user jiffies)
    ///   - 15: stime (system jiffies)
    ///   - 20: num_threads
    ///   - 24: rss (pages)
    ///
    /// The comm field (field 2) may contain spaces and parentheses, so we
    /// locate the last `)` to find the end of that field before splitting
    /// the remaining columns.
    fn parse_proc_stat(pid: u32, snap: &mut JobSnapshot) -> Result<(), Box<dyn Error>> {
        let raw = fs::read_to_string(format!("/proc/{pid}/stat"))?;

        // The comm field is wrapped in parens and may contain spaces/parens.
        // Find the last ')' to reliably split the remaining fields.
        let close_paren = raw
            .rfind(')')
            .ok_or_else(|| format!("malformed /proc/{pid}/stat: no closing paren"))?;

        let rest = &raw[close_paren + 2..]; // skip ") "
        let fields: Vec<&str> = rest.split_whitespace().collect();

        // After the comm field, field indices are offset by 2 (pid and comm).
        // So field 14 (utime) is at index 11, etc.
        if fields.len() < 22 {
            return Err(format!("/proc/{pid}/stat: not enough fields").into());
        }

        let utime: u64 = fields[11].parse()?; // field 14
        let stime: u64 = fields[12].parse()?; // field 15
        let num_threads: u64 = fields[17].parse()?; // field 20
        let rss: u64 = fields[21].parse()?; // field 24

        snap.cpu.utime += utime;
        snap.cpu.stime += stime;
        snap.num_threads += num_threads;
        snap.rss_pages += rss;

        Ok(())
    }

    /// Parse `/proc/[pid]/io` and accumulate read/write byte counts.
    ///
    /// This file may not be readable without elevated privileges. If it
    /// cannot be read, the snapshot's I/O fields are left unchanged.
    fn parse_proc_io(pid: u32, snap: &mut JobSnapshot) {
        let Ok(content) = fs::read_to_string(format!("/proc/{pid}/io")) else {
            return;
        };

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 {
                continue;
            }

            match parts[0] {
                "read_bytes:" => {
                    if let Ok(v) = parts[1].parse::<u64>() {
                        *snap.io_read_bytes.get_or_insert(0) += v;
                    }
                }
                "write_bytes:" => {
                    if let Ok(v) = parts[1].parse::<u64>() {
                        *snap.io_write_bytes.get_or_insert(0) += v;
                    }
                }
                _ => {}
            }
        }
    }

    /// Return the system page size in bytes.
    fn page_size() -> u64 {
        // SAFETY: sysconf(_SC_PAGESIZE) is always safe and returns a positive value on Linux.
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 }
    }

    /// Return the number of clock ticks per second (USER_HZ).
    fn clock_ticks() -> f64 {
        // SAFETY: sysconf(_SC_CLK_TCK) is always safe.
        unsafe { libc::sysconf(libc::_SC_CLK_TCK) as f64 }
    }

    /// Collect snapshots for all jobs from the given process list.
    fn collect_snapshots(
        processes: &[HpcProcess],
    ) -> HashMap<(String, String), JobSnapshot> {
        let mut jobs: HashMap<(String, String), JobSnapshot> = HashMap::new();

        for proc in processes {
            let snap = jobs
                .entry((proc.jobid.clone(), proc.stepid.clone()))
                .or_default();

            if let Err(e) = Self::parse_proc_stat(proc.pid, snap) {
                eprintln!("warning: skipping pid {}: {e}", proc.pid);
                continue;
            }

            Self::parse_proc_io(proc.pid, snap);
        }

        jobs
    }
}

impl Profiler for SysJobProfiler {
    fn is_supported(&self) -> Result<(), String> {
        if !std::path::Path::new("/proc").exists() {
            return Err("SysJobProfiler: /proc filesystem not found".to_string());
        }
        Ok(())
    }

    /// Collect job-level system metrics by reading per-PID proc files
    /// and aggregating by (jobid, stepid).
    ///
    /// CPU percent metrics are computed as a delta from the previous scrape.
    /// On the first scrape, CPU percent metrics are omitted.
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<Metric>, Box<dyn Error>> {
        let now = Instant::now();
        let snapshots = Self::collect_snapshots(processes);

        let page_size = Self::page_size();
        let clk_tck = Self::clock_ticks();
        let mut metrics = Vec::new();

        // Extract current CPU samples for storage after metric generation
        let mut current_cpu: HashMap<(String, String), CpuSample> = HashMap::new();

        for ((jobid, stepid), snap) in &snapshots {
            let job_key = (jobid.clone(), stepid.clone());
            current_cpu.insert(job_key, snap.cpu.clone());

            let jobid_label = Some(jobid.clone());
            let stepid_label = Some(stepid.clone());

            // CPU percent (delta-based, omitted on first scrape)
            if let Some((prev_time, prev_samples)) = &self.prev_samples {
                if let Some(prev_cpu) = prev_samples.get(&(jobid.clone(), stepid.clone())) {
                    let wall_secs = now.duration_since(*prev_time).as_secs_f64();
                    if wall_secs > 0.0 {
                        let utime_delta = snap.cpu.utime.saturating_sub(prev_cpu.utime);
                        let stime_delta = snap.cpu.stime.saturating_sub(prev_cpu.stime);

                        let user_pct = (utime_delta as f64 / clk_tck) / wall_secs * 100.0;
                        let system_pct = (stime_delta as f64 / clk_tck) / wall_secs * 100.0;

                        metrics.push(Metric {
                            name: "job_cpu_user_percent",
                            jobid: jobid_label.clone(),
                            stepid: stepid_label.clone(),
                            value: user_pct,
                        });

                        metrics.push(Metric {
                            name: "job_cpu_system_percent",
                            jobid: jobid_label.clone(),
                            stepid: stepid_label.clone(),
                            value: system_pct,
                        });
                    }
                }
            }

            // Memory (instantaneous)
            metrics.push(Metric {
                name: "job_memory_used_bytes",
                jobid: jobid_label.clone(),
                stepid: stepid_label.clone(),
                value: (snap.rss_pages * page_size) as f64,
            });

            // Threads (instantaneous)
            metrics.push(Metric {
                name: "job_num_threads",
                jobid: jobid_label.clone(),
                stepid: stepid_label.clone(),
                value: snap.num_threads as f64,
            });

            // I/O (instantaneous, omitted if unavailable)
            if let Some(read_bytes) = snap.io_read_bytes {
                metrics.push(Metric {
                    name: "job_io_read_bytes",
                    jobid: jobid_label.clone(),
                    stepid: stepid_label.clone(),
                    value: read_bytes as f64,
                });
            }

            if let Some(write_bytes) = snap.io_write_bytes {
                metrics.push(Metric {
                    name: "job_io_write_bytes",
                    jobid: jobid_label.clone(),
                    stepid: stepid_label.clone(),
                    value: write_bytes as f64,
                });
            }
        }

        // Store current samples for next scrape
        self.prev_samples = Some((now, current_cpu));

        Ok(metrics)
    }
}
