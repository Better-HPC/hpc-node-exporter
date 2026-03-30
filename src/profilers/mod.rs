//! Profiler trait and shared metric types.
//!
//! This module defines common traits and types for implementing
//! hardware-specific profilers.

pub mod default;
pub mod nvidia;
pub mod system;

use std::error::Error;
use std::sync::LazyLock;

use crate::schedulers::HpcProcess;

/// The local hostname, resolved once on first access.
pub static HOSTNAME: LazyLock<String> =
    LazyLock::new(|| gethostname::gethostname().to_string_lossy().into_owned());

/// A single node telemetry measurement.
///
/// Each `Metric` carries a metric name, a set of key-value labels, and the
/// observed numeric value.
#[derive(Debug)]
pub struct Metric {
    pub name: &'static str,
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

impl Metric {
    /// Escape Prometheus label text.
    ///
    /// Prometheus requires label values to be enclosed in double quotes.
    /// and for backslashes, double quotes, and newlines to be escaped.
    fn escape_label_value(v: &str) -> String {
        v.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Return the metric in Prometheus line format.
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

/// Trait for collecting hardware telemetry metrics.
///
/// Implementors are responsible for gathering metrics from a specific
/// hardware domain and scope (e.g., CPU metrics, GPU card metrics).
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
