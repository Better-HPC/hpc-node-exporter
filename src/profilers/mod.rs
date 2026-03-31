//! Traits and types for hardware-specific profilers.
//!
//! Defines a common interface for collecting telemetry across heterogeneous
//! hardware backends. Each [`Profiler`] translates device- or system-specific
//! measurements into a uniform set of [`Metric`] values exportable in
//! Prometheus format.

pub mod default;
pub mod nvidia;
pub mod system;

use std::error::Error;
use std::sync::LazyLock;

use crate::schedulers::HpcProcess;

/// The local hostname, resolved once at startup.
pub static HOSTNAME: LazyLock<String> =
    LazyLock::new(|| gethostname::gethostname().to_string_lossy().into_owned());

/// A single telemetry measurement in Prometheus line format.
///
/// Carries a metric name, key-value labels, and an observed numeric value.
#[derive(Debug)]
pub struct Metric {
    pub name: &'static str,
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

impl Metric {
    /// Escapes a label value for Prometheus text exposition.
    ///
    /// Backslashes, double quotes, and newlines are escaped per the
    /// Prometheus specification.
    fn escape_label_value(label: &str) -> String {
        label
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Renders the metric as a Prometheus text exposition line.
    pub fn to_prometheus(&self) -> String {
        // Render individual label/value pairs as strings
        let mut parts = Vec::with_capacity(self.labels.len());
        for (k, v) in &self.labels {
            parts.push(format!(r#"{k}="{}""#, Self::escape_label_value(v)));
        }

        // Combine labels and value into a single line
        let labels_str = parts.join(",");
        format!(
            "{name}{{{labels_str}}} {val:.1}",
            name = self.name,
            val = self.value
        )
    }
}

/// A collector of hardware telemetry metrics.
///
/// Implementors are responsible for gathering metrics from a specific
/// hardware domain and scope (e.g., CPU metrics, GPU card metrics).
pub trait Profiler {
    /// Collects current metrics for the given HPC `processes`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying hardware interface fails irrecoverably.
    fn collect_metrics(&mut self, processes: &[HpcProcess]) -> Result<Vec<Metric>, Box<dyn Error>>;
}
