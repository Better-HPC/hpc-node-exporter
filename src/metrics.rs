//! Core types and structures for Prometheus metrics.
//!
//! Each hardware measurement is recorded as a [`MetricSample`], which pair a
//! numeric value with a set of identifying labels. Related samples are
//! grouped into a [`MetricFamily`], combining the samples with a shared name,
//! help string, and [`MetricType`].

/// The Prometheus metric type.
#[derive(Debug, Clone, Copy)]
pub enum MetricType {
    Counter, // Always increasing
    Gauge,   // Can increase or decrease
}

impl MetricType {
    fn as_str(self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
        }
    }
}

/// A single metric value.
#[derive(Debug)]
pub struct MetricSample {
    pub labels: Vec<(&'static str, String)>,
    pub value: f64,
}

/// A collection of related Prometheus metrics.
///
/// All samples share the same metric name, type, and help string. Labels
/// differentiate individual samples within the family.
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
                "{name}{{{labels}}} {value:.4}\n",
                name = self.name,
                labels = labels.join(","),
                value = sample.value,
            ));
        }

        out
    }
}
