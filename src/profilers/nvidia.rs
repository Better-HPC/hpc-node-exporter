//! Hardware profiler for NVIDIA GPUs.
//!
//! Uses [`nvml_wrapper`] to collect per-card and per-job GPU telemetry
//! including utilization, memory, temperature, power, clocks, and fan speed.

use std::collections::HashMap;
use std::error::Error;

use log::warn;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::Nvml;

use crate::profilers::{Metric, Profiler, HOSTNAME};
use crate::schedulers::HpcProcess;

/// Aggregated per-job GPU usage across one or more devices.
#[derive(Debug, Default)]
struct NvidiaJobSnapshot {
    memory_bytes: u64,
    process_count: u32,
}

/// A [`Profiler`] for NVIDIA GPU metrics.
#[derive(Debug)]
pub struct NvidiaProfiler {
    nvml: Nvml,
}

impl NvidiaProfiler {
    /// Initializes the NVML library and returns a new profiler.
    ///
    /// # Errors
    ///
    /// Returns an error if the NVIDIA driver or NVML shared library is
    /// unavailable, initialization fails, or no GPU devices are detected.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        // Use a long-lived `Nvml` handle to ensure consistent deltas
        // between successive measurements.
        let nvml = Nvml::init()?;

        let count = nvml.device_count()?;
        if count == 0 {
            return Err("NvidiaProfiler: no NVIDIA GPU devices found".into());
        }

        Ok(Self { nvml })
    }

    /// Returns common labels for card-level metrics.
    fn gpu_labels(gpu_uuid: &str) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("gpu_uuid", gpu_uuid.to_string()),
        ]
    }

    /// Returns common labels for job-level metrics.
    fn job_labels(jobid: &str, stepid: &str, gpu_uuid: &str) -> Vec<(&'static str, String)> {
        vec![
            ("hostname", HOSTNAME.clone()),
            ("jobid", jobid.to_string()),
            ("stepid", stepid.to_string()),
            ("gpu_uuid", gpu_uuid.to_string()),
        ]
    }
}

impl Profiler for NvidiaProfiler {
    /// Measures and returns all GPU usage metrics.
    ///
    /// Individual device or process query failures are logged as warnings
    /// and skipped; an error is returned only on a fundamental NVML failure.
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>> {
        let mut metrics = Vec::new();

        let count = match self.nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to get GPU device count: {e}");
                return Ok(metrics);
            }
        };

        // Build a PID → (jobid, stepid) lookup for O(1) matching.
        let pid_to_job: HashMap<u32, (&str, &str)> = processes
            .iter()
            .map(|p| (p.pid, (p.jobid.as_str(), p.stepid.as_str())))
            .collect();

        // Accumulate per-job metrics across devices.
        let mut snapshots: HashMap<(String, String, String), NvidiaJobSnapshot> = HashMap::new();

        for i in 0..count {
            let device = match self.nvml.device_by_index(i) {
                Ok(d) => d,
                Err(e) => {
                    warn!("failed to get GPU device {i}: {e}");
                    continue;
                }
            };

            let uuid = match device.uuid() {
                Ok(u) => u,
                Err(e) => {
                    warn!("failed to get UUID for GPU {i}: {e}");
                    continue;
                }
            };

            let labels = Self::gpu_labels(&uuid);

            // --- Node-level metrics ---

            if let Ok(util) = device.utilization_rates() {
                metrics.push(Metric {
                    name: "kys_gpu_utilization_percent",
                    labels: labels.clone(),
                    value: util.gpu as f64,
                });
                metrics.push(Metric {
                    name: "kys_gpu_memory_utilization_percent",
                    labels: labels.clone(),
                    value: util.memory as f64,
                });
            }

            if let Ok(mem) = device.memory_info() {
                metrics.push(Metric {
                    name: "kys_gpu_memory_total_bytes",
                    labels: labels.clone(),
                    value: mem.total as f64,
                });
                metrics.push(Metric {
                    name: "kys_gpu_memory_used_bytes",
                    labels: labels.clone(),
                    value: mem.used as f64,
                });
                metrics.push(Metric {
                    name: "kys_gpu_memory_free_bytes",
                    labels: labels.clone(),
                    value: mem.free as f64,
                });
            }

            if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                metrics.push(Metric {
                    name: "kys_gpu_temperature_celsius",
                    labels: labels.clone(),
                    value: temp as f64,
                });
            }

            if let Ok(power) = device.power_usage() {
                metrics.push(Metric {
                    name: "kys_gpu_power_usage_watts",
                    labels: labels.clone(),
                    value: power as f64 / 1000.0,
                });
            }

            if let Ok(clock) = device.clock_info(Clock::Graphics) {
                metrics.push(Metric {
                    name: "kys_gpu_clock_graphics_mhz",
                    labels: labels.clone(),
                    value: clock as f64,
                });
            }

            if let Ok(clock) = device.clock_info(Clock::Memory) {
                metrics.push(Metric {
                    name: "kys_gpu_clock_memory_mhz",
                    labels: labels.clone(),
                    value: clock as f64,
                });
            }

            if let Ok(fan) = device.fan_speed(0) {
                metrics.push(Metric {
                    name: "kys_gpu_fan_speed_percent",
                    labels: labels.clone(),
                    value: fan as f64,
                });
            }

            // --- Per-job memory from compute processes on this device ---

            let gpu_procs = match device.running_compute_processes() {
                Ok(p) => p,
                Err(e) => {
                    warn!("failed to get compute processes for GPU {i}: {e}");
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
                    UsedGpuMemory::Used(bytes) => bytes,
                    UsedGpuMemory::Unavailable => 0,
                };
                snap.memory_bytes += mem;
                snap.process_count += 1;
            }
        }

        // Flatten job snapshots into metrics.
        for ((jobid, stepid, gpu_uuid), snap) in &snapshots {
            let labels = Self::job_labels(jobid, stepid, gpu_uuid);

            metrics.push(Metric {
                name: "kys_job_gpu_memory_used_bytes",
                labels: labels.clone(),
                value: snap.memory_bytes as f64,
            });

            metrics.push(Metric {
                name: "kys_job_gpu_process_count",
                labels,
                value: snap.process_count as f64,
            });
        }

        Ok(metrics)
    }
}
