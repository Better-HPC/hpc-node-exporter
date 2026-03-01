//! Combined node-level and job-level NVIDIA GPU profiler.
//!
//! This module provides [`NvidiaProfiler`], which uses the [`nvml_wrapper`]
//! crate (safe Rust bindings for NVIDIA's Management Library) to collect
//! telemetry for Nvidia GPU utilization.

use std::collections::HashMap;
use std::error::Error;

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;

use crate::profilers::{Metric, Profiler};
use crate::schedulers::HpcProcess;

/// Aggregated per-job GPU memory usage across one or more devices.
///
/// Built by summing `used_gpu_memory` for every compute process whose PID
/// matches a scheduler-reported [`HpcProcess`] under the same `(jobid, stepid)`
/// key.
#[derive(Debug, Default)]
struct GpuJobSnapshot {
    memory_bytes: u64,
}

/// A [`Profiler`] that collects NVIDIA GPU metrics.
///
/// Holds a long-lived [`Nvml`] handle that is initialized once at
/// construction time. The NVML library is loaded dynamically, so the
/// binary can run (and skip GPU metrics) on nodes without NVIDIA hardware.
#[derive(Debug)]
pub struct NvidiaProfiler {
    nvml: Nvml,
}

impl NvidiaProfiler {
    /// Initialize the NVML library and return a new profiler instance.
    ///
    /// Calls [`Nvml::init`] to dynamically load `libnvidia-ml.so` and
    /// resolve its function symbols. This is intentionally separated from
    /// the [`Profiler`] trait so callers can handle initialization failure
    /// before registering the profiler.
    ///
    /// # Errors
    ///
    /// Returns an error if the NVIDIA driver is not installed, the NVML
    /// shared library cannot be found, or the library fails to initialize.
    pub fn new() -> Result<Self, nvml_wrapper::error::NvmlError> {
        let nvml = Nvml::init()?;
        Ok(Self { nvml })
    }

    /// Collect per-device GPU utilization and memory metrics.
    ///
    /// Iterates over all devices reported by [`Nvml::device_count`] and
    /// queries each for its UUID, compute utilization percentage, and
    /// memory allocation breakdown (total, used, free).
    ///
    /// If a device query fails (e.g., the GPU falls off the bus mid-scrape),
    /// a warning is printed and that device is skipped rather than failing
    /// the entire collection.
    fn collect_utilization_and_memory(&self) -> Vec<Metric> {
        let mut metrics = Vec::new();

        let count = match self.nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to get GPU device count: {e}");
                return metrics;
            }
        };

        for i in 0..count {
            let device = match self.nvml.device_by_index(i) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("warning: failed to get GPU device {i}: {e}");
                    continue;
                }
            };

            let uuid = match device.uuid() {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("warning: failed to get UUID for GPU {i}: {e}");
                    continue;
                }
            };

            let labels = vec![("gpu_uuid", uuid.clone())];

            if let Ok(util) = device.utilization_rates() {
                metrics.push(Metric {
                    name: "gpu_utilization_percent",
                    labels: labels.clone(),
                    value: util.gpu as f64,
                });
            }

            if let Ok(mem) = device.memory_info() {
                metrics.push(Metric {
                    name: "gpu_memory_total_bytes",
                    labels: labels.clone(),
                    value: mem.total as f64,
                });
                metrics.push(Metric {
                    name: "gpu_memory_used_bytes",
                    labels: labels.clone(),
                    value: mem.used as f64,
                });
                metrics.push(Metric {
                    name: "gpu_memory_free_bytes",
                    labels: labels.clone(),
                    value: mem.free as f64,
                });
            }

            if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                metrics.push(Metric {
                    name: "gpu_temperature_celsius",
                    labels: labels.clone(),
                    value: temp as f64,
                });
            }

            if let Ok(power) = device.power_usage() {
                metrics.push(Metric {
                    name: "gpu_power_usage_milliwatts",
                    labels,
                    value: power as f64,
                });
            }
        }

        metrics
    }

    // ── Job-level collectors ────────────────────────────────────────────

    /// Build per-job GPU memory snapshots by matching compute process PIDs
    /// against the scheduler's process list.
    ///
    /// For each GPU device, this method:
    ///
    /// 1. Queries NVML for the list of running compute processes via
    ///    [`Device::running_compute_processes`], which returns each
    ///    process's PID and GPU memory usage.
    /// 2. Builds a lookup table from the scheduler's [`HpcProcess`] list
    ///    to quickly map PIDs to `(jobid, stepid)` pairs.
    /// 3. For each GPU process whose PID appears in the lookup table,
    ///    accumulates its GPU memory usage into the corresponding
    ///    [`GpuJobSnapshot`], keyed by `(jobid, stepid, gpu_uuid)`.
    ///
    /// Processes not found in the scheduler list (e.g., system daemons
    /// using the GPU) are silently ignored.
    ///
    /// # Arguments
    ///
    /// * `processes` — Active processes discovered by the HPC scheduler.
    ///
    /// # Returns
    ///
    /// A map from `(jobid, stepid, gpu_uuid)` to the aggregated
    /// [`GpuJobSnapshot`] for that job step on that device.
    fn collect_job_snapshots(
        &self,
        processes: &[HpcProcess],
    ) -> HashMap<(String, String, String), GpuJobSnapshot> {
        // Build a PID → (jobid, stepid) lookup for O(1) matching
        let pid_to_job: HashMap<u32, (&str, &str)> = processes
            .iter()
            .map(|p| (p.pid, (p.jobid.as_str(), p.stepid.as_str())))
            .collect();

        let mut snapshots: HashMap<(String, String, String), GpuJobSnapshot> = HashMap::new();

        let count = match self.nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to get GPU device count: {e}");
                return snapshots;
            }
        };

        for i in 0..count {
            let device = match self.nvml.device_by_index(i) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("warning: failed to get GPU device {i}: {e}");
                    continue;
                }
            };

            let uuid = match device.uuid() {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("warning: failed to get UUID for GPU {i}: {e}");
                    continue;
                }
            };

            let gpu_procs = match device.running_compute_processes() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("warning: failed to get compute processes for GPU {i}: {e}");
                    continue;
                }
            };

            for proc_info in &gpu_procs {
                let Some(&(jobid, stepid)) = pid_to_job.get(&proc_info.pid) else {
                    continue;
                };

                let snap = snapshots
                    .entry((jobid.to_string(), stepid.to_string(), uuid.clone()))
                    .or_default();

                let mem = match proc_info.used_gpu_memory {
                    nvml_wrapper::enums::device::UsedGpuMemory::Used(bytes) => bytes,
                    nvml_wrapper::enums::device::UsedGpuMemory::Unavailable => 0,
                };
                snap.memory_bytes += mem;
            }
        }

        snapshots
    }

    /// Convert per-job GPU snapshots into Prometheus-ready [`Metric`] values.
    ///
    /// Delegates to [`collect_job_snapshots`](Self::collect_job_snapshots)
    /// to build the aggregated data, then produces one `job_gpu_memory_used_bytes`
    /// metric per `(jobid, stepid, gpu_uuid)` combination.
    ///
    /// # Arguments
    ///
    /// * `processes` — Active processes discovered by the HPC scheduler.
    fn collect_job_metrics(&self, processes: &[HpcProcess]) -> Vec<Metric> {
        let snapshots = self.collect_job_snapshots(processes);
        let mut metrics = Vec::new();

        for ((jobid, stepid, gpu_uuid), snap) in &snapshots {
            metrics.push(Metric {
                name: "job_gpu_memory_used_bytes",
                labels: vec![
                    ("jobid", jobid.clone()),
                    ("stepid", stepid.clone()),
                    ("gpu_uuid", gpu_uuid.clone()),
                ],
                value: snap.memory_bytes as f64,
            });
        }

        metrics
    }
}

impl Profiler for NvidiaProfiler {
    /// Check whether the NVML library was successfully initialized.
    ///
    /// Since [`NvidiaProfiler::new`] already performs initialization,
    /// this method validates that the library can enumerate at least one
    /// GPU device. A system with the NVIDIA driver installed but no GPUs
    /// visible (e.g., inside a container without device passthrough) will
    /// fail this check.
    fn is_supported(&self) -> Result<(), String> {
        let count = self
            .nvml
            .device_count()
            .map_err(|e| format!("NvidiaProfiler: failed to query device count: {e}"))?;

        if count == 0 {
            return Err("NvidiaProfiler: no NVIDIA GPU devices found".to_string());
        }

        Ok(())
    }

    /// Collect all GPU metrics — both node-level and job-level — in a
    /// single pass.
    ///
    /// Node-level metrics are collected first (utilization, memory,
    /// temperature, power for each device), followed by job-level metrics
    /// (GPU memory attributed to scheduler jobs via PID matching).
    ///
    /// # Arguments
    ///
    /// * `processes` — Active HPC processes on this node, as reported by
    ///   the scheduler. Used for job-level GPU memory attribution.
    ///
    /// # Errors
    ///
    /// Returns an error if a fundamental NVML failure occurs. Individual
    /// device or process query failures are logged as warnings and skipped
    /// rather than propagated.
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<Metric>, Box<dyn Error>> {
        let mut metrics = Vec::new();
        metrics.extend(self.collect_utilization_and_memory());
        metrics.extend(self.collect_job_metrics(processes));
        Ok(metrics)
    }
}
