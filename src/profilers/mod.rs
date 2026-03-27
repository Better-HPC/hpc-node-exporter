//! Profiler trait and shared metric types.
//!
//! This module defines traits and types used by all hardware-specific profiler implementations.

pub mod nvidia;
pub mod system;

use std::error::Error;
use std::sync::OnceLock;

use crate::schedulers::HpcProcess;

/// A single node telemetry measurement.
///
/// Each `Metric` carries a metric name, a set of key-value labels, and the observed numeric value.
/// The hostname for the parent system is automatically injected into the label values when rendered
/// to Prometheus format. All other labels must be specified manually.
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

    /// Escape Prometheus label text.
    ///
    /// Prometheus requires label values to be enclosed in double quotes
    /// and for backslashes, double quotes, and newlines to be escaped.
    fn escape_label_value(v: &str) -> String {
        v.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Return the metric in Prometheus line format.
    pub fn to_prometheus(&self) -> String {
        let host = Self::hostname();

        // Render individual label/value pairs as strings
        let mut parts = vec![format!(r#"hostname="{}""#, Self::escape_label_value(host))];
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

/// Trait for collecting hardware telemetry metrics.
///
/// Implementors are responsible for gathering metrics from a specific
/// hardware domain and scope (e.g., CPU/memory system metrics, GPU card metrics).
pub trait Profiler {
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
