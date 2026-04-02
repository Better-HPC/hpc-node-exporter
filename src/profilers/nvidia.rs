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

use crate::metrics::{MetricFamily, MetricType};
use crate::profilers::{Profiler, HOSTNAME};
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
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<MetricFamily>, Box<dyn Error>> {
        let count = match self.nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to get GPU device count: {e}");
                return Ok(Vec::new());
            }
        };

        // Declare all card-level families up front so each device's
        // samples land in the correct shared family.
        let mut gpu_util = MetricFamily::new(
            "hpcexp_gpu_utilization_percent",
            "GPU core utilization as a percentage.",
            MetricType::Gauge,
        );
        let mut mem_util = MetricFamily::new(
            "hpcexp_gpu_memory_utilization_percent",
            "GPU memory controller utilization as a percentage.",
            MetricType::Gauge,
        );
        let mut mem_total = MetricFamily::new(
            "hpcexp_gpu_memory_total_bytes",
            "Total GPU memory on this device in bytes.",
            MetricType::Gauge,
        );
        let mut mem_used = MetricFamily::new(
            "hpcexp_gpu_memory_used_bytes",
            "GPU memory currently in use on this device in bytes.",
            MetricType::Gauge,
        );
        let mut mem_free = MetricFamily::new(
            "hpcexp_gpu_memory_free_bytes",
            "GPU memory currently free on this device in bytes.",
            MetricType::Gauge,
        );
        let mut temp = MetricFamily::new(
            "hpcexp_gpu_temperature_celsius",
            "GPU core temperature in degrees Celsius.",
            MetricType::Gauge,
        );
        let mut power = MetricFamily::new(
            "hpcexp_gpu_power_usage_watts",
            "GPU power draw in watts.",
            MetricType::Gauge,
        );
        let mut clock_graphics = MetricFamily::new(
            "hpcexp_gpu_clock_graphics_mhz",
            "Current GPU graphics clock speed in MHz.",
            MetricType::Gauge,
        );
        let mut clock_memory = MetricFamily::new(
            "hpcexp_gpu_clock_memory_mhz",
            "Current GPU memory clock speed in MHz.",
            MetricType::Gauge,
        );
        let mut fan = MetricFamily::new(
            "hpcexp_gpu_fan_speed_percent",
            "GPU fan speed as a percentage of maximum.",
            MetricType::Gauge,
        );

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

            if let Ok(util) = device.utilization_rates() {
                gpu_util.add(labels.clone(), util.gpu as f64);
                mem_util.add(labels.clone(), util.memory as f64);
            }

            if let Ok(info) = device.memory_info() {
                mem_total.add(labels.clone(), info.total as f64);
                mem_used.add(labels.clone(), info.used as f64);
                mem_free.add(labels.clone(), info.free as f64);
            }

            if let Ok(t) = device.temperature(TemperatureSensor::Gpu) {
                temp.add(labels.clone(), t as f64);
            }

            if let Ok(p) = device.power_usage() {
                power.add(labels.clone(), p as f64 / 1000.0);
            }

            if let Ok(c) = device.clock_info(Clock::Graphics) {
                clock_graphics.add(labels.clone(), c as f64);
            }

            if let Ok(c) = device.clock_info(Clock::Memory) {
                clock_memory.add(labels.clone(), c as f64);
            }

            if let Ok(f) = device.fan_speed(0) {
                fan.add(labels, f as f64);
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

                snap.memory_bytes += match proc_info.used_gpu_memory {
                    UsedGpuMemory::Used(bytes) => bytes,
                    UsedGpuMemory::Unavailable => 0,
                };
                snap.process_count += 1;
            }
        }

        // Flatten job snapshots into their families.
        let mut job_mem = MetricFamily::new(
            "hpcexp_gpu_job_memory_used_bytes",
            "GPU memory used by an HPC job step on a specific device, in bytes.",
            MetricType::Gauge,
        );
        let mut job_procs = MetricFamily::new(
            "hpcexp_gpu_job_process_count",
            "Number of processes belonging to an HPC job step running on a specific GPU.",
            MetricType::Gauge,
        );

        for ((jobid, stepid, gpu_uuid), snap) in &snapshots {
            let labels = Self::job_labels(jobid, stepid, gpu_uuid);
            job_mem.add(labels.clone(), snap.memory_bytes as f64);
            job_procs.add(labels, snap.process_count as f64);
        }

        Ok(vec![
            gpu_util, mem_util, mem_total, mem_used, mem_free,
            temp, power, clock_graphics, clock_memory, fan,
            job_mem, job_procs,
        ])
    }
}
