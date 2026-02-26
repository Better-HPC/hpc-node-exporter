pub mod sys_node;

use std::error::Error;
use std::sync::OnceLock;

use crate::schedulers::HpcProcess;

/// A single node telemetry measurement.
///
/// Each `Metric` carries a metric name, the hostname it was collected on,
/// an optional Slurm job/step identifier for job-level attribution, and
/// the observed numeric value.
#[derive(Debug)]
pub struct Metric {
    pub name: &'static str,
    pub jobid: Option<String>,
    pub stepid: Option<String>,
    pub value: f64,
}

impl Metric {
    /// Return the local hostname, resolved once and cached for the process lifetime.
    fn hostname() -> &'static str {
        static HOSTNAME: OnceLock<String> = OnceLock::new();
        HOSTNAME.get_or_init(|| gethostname::gethostname().to_string_lossy().into_owned())
    }

    /// Return the metric in Prometheus line format.
    pub fn to_prometheus(&self) -> String {
        let host = Self::hostname();

        match (&self.jobid, &self.stepid) {
            // Format job-level metrics with a job/step ID
            (Some(jobid), Some(stepid)) => {
                format!(
                    r#"{name}{{hostname="{host}",jobid="{job}",stepid="{step}"}} {val:.1}"#,
                    name = self.name,
                    job = jobid,
                    step = stepid,
                    val = self.value,
                )
            }

            // Format system-level metrics without a job/step ID
            _ => {
                format!(
                    r#"{name}{{hostname="{host}"}} {val:.1}"#,
                    name = self.name,
                    val = self.value,
                )
            }
        }
    }
}

/// Trait for collecting hardware telemetry metrics.
///
/// Implementors are responsible for gathering metrics from a specific
/// hardware domain and scope. hardware domain (e.g., CPU/memory system
/// metrics, NVIDIA GPU metrics).
pub trait Profiler {
    /// Check whether this profiler is supported on the current system.
    /// Implementors should check for the presence of required drivers, tools, and interfaces.
    fn is_supported(&self) -> Result<(), String>;

    /// Collect metrics and return them as a vector of [`Metric`] values.
    ///
    /// # Arguments
    ///
    /// * `processes` - The active HPC processes running on the host machine.
    fn collect_metrics(&self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>>;
}
