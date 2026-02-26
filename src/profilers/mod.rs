/// A single node telemetry measurement.
///
/// Each `Metric` carries a metric name, the hostname it was collected on,
/// an optional Slurm job/step identifier for job-level attribution, and
/// the observed numeric value.
#[derive(Debug)]
pub struct Metric {
    pub name: &'static str,
    pub hostname: Arc<str>,
    pub jobid: Option<String>,
    pub stepid: Option<String>,
    pub value: f64,
}

impl Metric {
    /// Return the metric in Prometheus line format.
    pub fn to_prometheus(&self) -> String {
        match (&self.jobid, &self.stepid) {
            // Format job-level metrics with a job/step ID
            (Some(jobid), Some(stepid)) => {
                format!(
                    r#"{name}{{hostname="{host}",jobid="{job}",stepid="{step}"}} {val:.1}"#,
                    name = self.name,
                    host = self.hostname,
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
                    host = self.hostname,
                    val = self.value,
                )
            }
        }
    }
}
