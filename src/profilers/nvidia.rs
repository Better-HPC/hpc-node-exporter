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
/// Represents GPU usage summed over all active process running under the same `(jobid, stepid)`.
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
    /// resolve its function symbols, then verifies that at least one GPU
    /// device is visible. A system with the NVIDIA driver installed but no
    /// GPUs accessible (e.g., inside a container without device passthrough)
    /// will fail this check.
    ///
    /// # Returns
    ///
    /// A new [`NvidiaProfiler`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the NVIDIA driver is not installed, the NVML
    /// shared library cannot be found, the library fails to initialize,
    /// or no GPU devices are detected.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let nvml = Nvml::init()?;

        let count = nvml.device_count()?;
        if count == 0 {
            return Err("NvidiaProfiler: no NVIDIA GPU devices found".into());
        }

        Ok(Self { nvml })
    }

    /// Collect metrics for all GPUs and HPC jobs in a single pass.
    ///
    /// For each device, collects both node-level telemetry (utilization,
    /// memory, temperature, power) and per-job GPU memory usage by matching
    /// running compute process PIDs against the scheduler's process list.
    ///
    /// This avoids enumerating the device list multiple times per scrape.
    /// If a device query fails (e.g., the GPU falls off the bus mid-scrape),
    /// a warning is printed and that device is skipped rather than failing
    /// the entire collection.
    ///
    /// # Arguments
    ///
    /// * `processes` - Active system processes to collect metrics for.
    ///
    /// # Returns
    ///
    /// A vector of profiling metrics including:
    /// `gpu_utilization_percent`, `gpu_memory_total_bytes`, `gpu_memory_used_bytes`,
    /// `gpu_memory_free_bytes`, `gpu_temperature_celsius`, `gpu_power_usage_watts`,
    /// and `job_gpu_memory_used_bytes`.
    fn collect_all(&mut self, processes: &[HpcProcess]) -> Vec<Metric> {
        let mut metrics = Vec::new();

        let count = match self.nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to get GPU device count: {e}");
                return metrics;
            }
        };

        // Build a PID → (jobid, stepid) lookup for O(1) matching
        let pid_to_job: HashMap<u32, (&str, &str)> = processes
            .iter()
            .map(|p| (p.pid, (p.jobid.as_str(), p.stepid.as_str())))
            .collect();

        // Accumulate per-job memory across devices
        let mut snapshots: HashMap<(String, String, String), GpuJobSnapshot> = HashMap::new();

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

            // --- Node-level metrics ---

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
                    name: "gpu_power_usage_watts",
                    labels,
                    value: power as f64 / 1000.0,
                });
            }

            // --- Per-job memory from compute processes on this device ---

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

        // Flatten job snapshots into metrics
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
    /// Collect all card and job-level metrics for NVIDIA GPUs.
    ///
    /// Performs a single pass over all GPU devices, collecting both
    /// node-level and per-job metrics in one enumeration.
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
    /// Returns an error if a fundamental NVML failure occurs. Individual
    /// device or process query failures are logged as warnings and skipped
    /// rather than propagated.
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<Metric>, Box<dyn Error>> {
        Ok(self.collect_all(processes))
    }
}
