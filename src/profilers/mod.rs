//! Traits and types for hardware-specific profilers.
//!
//! Defines a common interface for collecting telemetry across heterogeneous
//! hardware backends. Each [`Profiler`] translates device- or system-specific
//! measurements into a uniform set of [`MetricFamily`] values exportable in
//! Prometheus text exposition format.

pub mod default;
pub mod nvidia;
pub mod system;

use std::error::Error;
use std::sync::LazyLock;

use crate::metrics::MetricFamily;
use crate::schedulers::HpcProcess;

/// The local hostname, resolved once at startup.
pub static HOSTNAME: LazyLock<String> =
    LazyLock::new(|| gethostname::gethostname().to_string_lossy().into_owned());

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
    fn collect_metrics(
        &mut self,
        processes: &[HpcProcess],
    ) -> Result<Vec<MetricFamily>, Box<dyn Error>>;
}
