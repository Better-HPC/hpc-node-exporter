//! Node-level system profiler.
//!
//! Collects machine-wide telemetry from `/proc` virtual files, independent
//! of any particular HPC job. Metrics are read from the following sources:
//!
//! - `/proc/stat` — per-CPU utilization (user, system, idle percentages)
//! - `/proc/meminfo` — memory usage (total, used, available bytes)
//! - `/proc/diskstats` — disk I/O (read/written bytes per device, delta between scrapes)
//! - `/proc/net/dev` — network I/O (rx/tx bytes per interface)

use std::collections::HashMap;
use std::error::Error;
use std::fs;

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// Per-device cumulative disk counters from the previous scrape.
#[derive(Debug, Clone)]
struct DiskSample {
    sectors_read: f64,
    sectors_written: f64,
}

/// A [`Profiler`] that collects node-level system metrics from `/proc`.
#[derive(Debug, Default)]
pub struct SysNodeProfiler {
    /// Previous disk counters keyed by device name, used for delta computation.
    prev_disk: Option<HashMap<String, DiskSample>>,
}

impl SysNodeProfiler {
    /// Parse `/proc/stat` and return per-CPU utilization metrics.
    ///
    /// Each `cpu<N>` line is parsed into user, system, and idle percentages
    /// based on the jiffy counters exposed by the kernel. The aggregate `cpu`
    /// line is skipped so that only individual cores are reported.
    fn collect_cpu() -> Result<Vec<Metric>, Box<dyn Error>> {
        let stat = fs::read_to_string("/proc/stat")?;
        let mut metrics = Vec::new();

        for line in stat.lines() {
            // Use only the aggregate "cpu" line (not per-core "cpuN" lines)
            if !line.starts_with("cpu ") {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 8 {
                continue;
            }

            let _label = parts[0]; // e.g. "cpu0"
            let user: f64 = parts[1].parse()?;
            let nice: f64 = parts[2].parse()?;
            let system: f64 = parts[3].parse()?;
            let idle: f64 = parts[4].parse()?;
            let iowait: f64 = parts[5].parse()?;
            let irq: f64 = parts[6].parse()?;
            let softirq: f64 = parts[7].parse()?;

            let total = user + nice + system + idle + iowait + irq + softirq;
            if total == 0.0 {
                continue;
            }

            let user_pct = (user + nice) / total * 100.0;
            let system_pct = (system + irq + softirq) / total * 100.0;
            let idle_pct = idle / total * 100.0;

            metrics.push(Metric {
                name: "node_cpu_user_percent",
                jobid: None,
                stepid: None,
                value: user_pct,
            });

            metrics.push(Metric {
                name: "node_cpu_system_percent",
                jobid: None,
                stepid: None,
                value: system_pct,
            });

            metrics.push(Metric {
                name: "node_cpu_idle_percent",
                jobid: None,
                stepid: None,
                value: idle_pct,
            });
        }

        Ok(metrics)
    }

    /// Parse `/proc/meminfo` and return memory usage metrics.
    ///
    /// Reports total, used, and available memory in bytes. Used memory is
    /// computed as `total - available`.
    fn collect_memory() -> Result<Vec<Metric>, Box<dyn Error>> {
        let meminfo = fs::read_to_string("/proc/meminfo")?;
        let mut total_kb: Option<f64> = None;
        let mut available_kb: Option<f64> = None;

        for line in meminfo.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            match parts[0] {
                "MemTotal:" => total_kb = parts[1].parse().ok(),
                "MemAvailable:" => available_kb = parts[1].parse().ok(),
                _ => {}
            }
        }

        let total_kb = total_kb.ok_or("MemTotal not found in /proc/meminfo")?;
        let available_kb = available_kb.ok_or("MemAvailable not found in /proc/meminfo")?;
        let used_kb = total_kb - available_kb;

        Ok(vec![
            Metric {
                name: "node_memory_total_bytes",
                jobid: None,
                stepid: None,
                value: total_kb * 1024.0,
            },
            Metric {
                name: "node_memory_used_bytes",
                jobid: None,
                stepid: None,
                value: used_kb * 1024.0,
            },
            Metric {
                name: "node_memory_available_bytes",
                jobid: None,
                stepid: None,
                value: available_kb * 1024.0,
            },
        ])
    }

    /// Parse `/proc/diskstats` and return the current cumulative counters
    /// keyed by device name.
    ///
    /// Only whole devices (e.g. `sda`, `nvme0n1`) are reported — partitions
    /// are skipped by filtering for entries with a minor number of `0`.
    fn read_disk_counters() -> Result<HashMap<String, DiskSample>, Box<dyn Error>> {
        let diskstats = fs::read_to_string("/proc/diskstats")?;
        let mut counters = HashMap::new();

        for line in diskstats.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 14 {
                continue;
            }

            let minor: u64 = parts[1].parse().unwrap_or(1);
            if minor != 0 {
                continue;
            }

            let device = parts[2].to_string();
            let sectors_read: f64 = parts[5].parse().unwrap_or(0.0);
            let sectors_written: f64 = parts[9].parse().unwrap_or(0.0);

            counters.insert(device, DiskSample { sectors_read, sectors_written });
        }

        Ok(counters)
    }

    /// Compute delta-based disk metrics from the current and previous samples.
    ///
    /// On the first scrape, the current counters are stored and no metrics
    /// are emitted. On subsequent scrapes, the delta is computed for each
    /// device and summed into a single read/write total.
    fn collect_disk(&mut self) -> Result<Vec<Metric>, Box<dyn Error>> {
        let current = Self::read_disk_counters()?;
        let mut metrics = Vec::new();

        if let Some(prev) = &self.prev_disk {
            let mut total_read: f64 = 0.0;
            let mut total_written: f64 = 0.0;

            for (device, curr_sample) in &current {
                if let Some(prev_sample) = prev.get(device) {
                    let read_delta = (curr_sample.sectors_read - prev_sample.sectors_read) * 512.0;
                    let write_delta =
                        (curr_sample.sectors_written - prev_sample.sectors_written) * 512.0;

                    // Guard against counter resets (e.g. device removal/re-add)
                    if read_delta >= 0.0 {
                        total_read += read_delta;
                    }
                    if write_delta >= 0.0 {
                        total_written += write_delta;
                    }
                }
            }

            metrics.push(Metric {
                name: "node_disk_read_bytes",
                jobid: None,
                stepid: None,
                value: total_read,
            });

            metrics.push(Metric {
                name: "node_disk_written_bytes",
                jobid: None,
                stepid: None,
                value: total_written,
            });
        }

        self.prev_disk = Some(current);
        Ok(metrics)
    }

    /// Parse `/proc/net/dev` and return aggregated network I/O metrics.
    ///
    /// Sums rx/tx bytes across all interfaces. The loopback interface (`lo`)
    /// is excluded.
    fn collect_network() -> Result<Vec<Metric>, Box<dyn Error>> {
        let netdev = fs::read_to_string("/proc/net/dev")?;
        let mut total_rx: f64 = 0.0;
        let mut total_tx: f64 = 0.0;

        for line in netdev.lines() {
            // Each interface line contains a colon separating the name from stats
            let Some((iface, rest)) = line.split_once(':') else {
                continue;
            };

            let iface = iface.trim();
            if iface == "lo" {
                continue;
            }

            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 10 {
                continue;
            }

            total_rx += parts[0].parse::<f64>().unwrap_or(0.0);
            total_tx += parts[8].parse::<f64>().unwrap_or(0.0);
        }

        Ok(vec![
            Metric {
                name: "node_net_rx_bytes",
                jobid: None,
                stepid: None,
                value: total_rx,
            },
            Metric {
                name: "node_net_tx_bytes",
                jobid: None,
                stepid: None,
                value: total_tx,
            },
        ])
    }
}

impl Profiler for SysNodeProfiler {
    /// System metrics are available on any Linux host with a `/proc` filesystem.
    fn is_supported(&self) -> Result<(), String> {
        let required = ["/proc/stat", "/proc/meminfo", "/proc/diskstats", "/proc/net/dev"];
        for path in required {
            if !std::path::Path::new(path).exists() {
                return Err(format!("SysNodeProfiler: required file {path} not found"));
            }
        }

        Ok(())
    }

    /// Collect node-level system metrics from `/proc`.
    ///
    /// The `processes` argument is ignored since node-level metrics are
    /// not attributed to individual jobs.
    fn collect_metrics(
        &mut self,
        _processes: &[HpcProcess],
    ) -> Result<Vec<Metric>, Box<dyn Error>> {
        let mut metrics = Vec::new();
        metrics.extend(Self::collect_cpu()?);
        metrics.extend(Self::collect_memory()?);
        metrics.extend(self.collect_disk()?);
        metrics.extend(Self::collect_network()?);
        Ok(metrics)
    }
}
