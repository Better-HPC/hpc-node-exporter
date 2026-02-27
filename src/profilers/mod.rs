pub mod sys_job;
pub mod sys_node;

use std::error::Error;
use std::sync::OnceLock;

use crate::schedulers::HpcProcess;

/// A single node telemetry measurement.
///
/// Each `Metric` carries a metric name, a set of key-value labels, and the observed numeric value.
/// The hostname for the parent system is automatically injected into the label values when rendered
/// to prometheus format. All other labels must be specified manually.
#[derive(Debug)]
pub struct Metric {
    pub name: &'static str,
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

impl Metric {
    /// Return the local hostname, resolved once and cached for the program lifetime.
    fn hostname() -> &'static str {
        static HOSTNAME: OnceLock<String> = OnceLock::new();
        HOSTNAME.get_or_init(|| gethostname::gethostname().to_string_lossy().into_owned())
    }

    /// Return the metric in Prometheus line format.
    pub fn to_prometheus(&self) -> String {
        let host = Self::hostname();

        // Render individual label/value pairs as strings
        let mut parts = vec![format!(r#"hostname="{host}""#)];
        for (k, v) in &self.labels {
            parts.push(format!(r#"{k}="{v}""#));
        }

        let labels_str = parts.join(",");
        format!(
            "{name}{{{labels_str}}} {val:.1}",
            name = self.name,
            val = self.value
        )
    }
}

/// Trait for collecting hardware telemetry metrics.
///
/// Implementors are responsible for gathering metrics from a specific
/// hardware domain and scope (e.g., CPU/memory system metrics, NVIDIA
/// GPU metrics).
pub trait Profiler {
    /// Check whether this profiler is supported on the current system.
    /// Implementors should check for the presence of required drivers, tools, and interfaces.
    fn is_supported(&self) -> Result<(), String>;

    /// Collect metrics and return them as a vector of [`Metric`] values.
    ///
    /// Takes `&mut self` to allow profilers to maintain state between
    /// collections (e.g., for computing deltas between scrapes).
    ///
    /// # Arguments
    ///
    /// * `processes` - The active HPC processes running on the host machine.
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>>;
}
