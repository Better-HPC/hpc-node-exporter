//! Core metric types for Prometheus text exposition format.
//!
//! Defines [`MetricFamily`], [`Sample`], and [`MetricType`] — the shared
//! data model used by all profilers and rendered by the background
//! collector.

/// The Prometheus metric type.
#[derive(Debug, Clone, Copy)]
pub enum MetricType {
    Counter,
    Gauge,
}

impl MetricType {
    fn as_str(self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
        }
    }
}

/// A single labeled sample within a [`MetricFamily`].
#[derive(Debug)]
pub struct MetricSample {
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

/// A Prometheus metric family: one `# HELP`, one `# TYPE`, and one or more samples.
///
/// All samples share the same metric name, type, and help string. Labels
/// differentiate individual time series within the family.
#[derive(Debug)]
pub struct MetricFamily {
    pub name: &'static str,
    pub help: &'static str,
    pub metric_type: MetricType,
    pub samples: Vec<MetricSample>,
}

impl MetricFamily {
    /// Creates a new family with an empty sample list.
    pub fn new(name: &'static str, help: &'static str, metric_type: MetricType) -> Self {
        Self {
            name,
            help,
            metric_type,
            samples: Vec::new(),
        }
    }

    /// Creates a new family pre-populated with `samples`.
    pub fn from_samples(
        name: &'static str,
        help: &'static str,
        metric_type: MetricType,
        samples: Vec<MetricSample>,
    ) -> Self {
        Self { name, help, metric_type, samples }
    }

    /// Appends a sample to this family.
    pub fn add(&mut self, labels: Vec<(&'static str, String)>, value: f64) {
        self.samples.push(MetricSample { labels, value });
    }

    /// Escapes a label value for Prometheus text exposition.
    ///
    /// Backslashes, double quotes, and newlines are escaped per the
    /// Prometheus specification.
    fn escape_label_value(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Renders the family as a Prometheus text exposition block.
    ///
    /// Emits a `# HELP` line, a `# TYPE` line, then one sample line per
    /// entry in `samples`. Returns an empty string if there are no samples.
    pub fn to_prometheus(&self) -> String {
        if self.samples.is_empty() {
            return String::new();
        }

        let mut out = format!(
            "# HELP {name} {help}\n# TYPE {name} {typ}\n",
            name = self.name,
            help = self.help,
            typ = self.metric_type.as_str(),
        );

        for sample in &self.samples {

            let labels: Vec<String> = sample
                .labels
                .iter()
                .map(|(k, v)| format!(r#"{k}="{}""#, Self::escape_label_value(v)))
                .collect();

            out.push_str(&format!(
                "{name}{{{labels}}} {value:.1}\n",
                name = self.name,
                labels = labels.join(","),
                value = sample.value,
            ));
        }

        out
    }
}
