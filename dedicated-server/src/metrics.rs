//! Prometheus-compatible metric registry for the dedicated server.
//!
//! The registry owns every label value it can render. Cluster node labels come
//! from the static manifest and are fixed at construction, and the
//! request-duration histogram uses the fixed v1 operation table. No scrape and
//! no request can introduce a new label value, which is what `NFR-OBS-008`
//! requires: labels never carry a login, `request_id`, `upload_id`, file name
//! or any other user-unique value.
//!
//! The rendered format is the Prometheus text exposition format served at
//! `GET /metrics` (`NFR-OBS-006`). Every metric and its labels are documented
//! in the area README.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// Gauge that is `1` while the process serves its metrics endpoint.
pub const METRIC_UP: &str = "homecloud_dedicated_up";

/// Gauge that is `1` while a configured storage node is considered available.
pub const METRIC_NODE_AVAILABLE: &str = "homecloud_dedicated_node_available";

/// Gauge that is `1` while a strict majority of the configured nodes is available.
pub const METRIC_QUORUM_AVAILABLE: &str = "homecloud_dedicated_quorum_available";

/// Counter of log records lost by the bounded writer (`NFR-OBS-004`).
pub const METRIC_LOG_DROPPED: &str = "homecloud_dedicated_log_dropped_records_total";

/// Gauge of the bounded log-writer queue capacity in records.
pub const METRIC_LOG_QUEUE_CAPACITY: &str = "homecloud_dedicated_log_queue_capacity_records";

/// Histogram of handled client request durations in seconds.
pub const METRIC_REQUEST_DURATION: &str = "homecloud_dedicated_request_duration_seconds";

/// Content type of the exposition format.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Client operations the dedicated server handles, used as bounded label values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Account and installation registration.
    Registration,
    /// Topology retrieval.
    Topology,
}

impl Operation {
    /// Every operation the dedicated server handles, in a fixed order.
    pub const ALL: [Self; 2] = [Self::Registration, Self::Topology];

    /// Label value for this operation.
    pub fn label(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::Topology => "topology",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Registration => 0,
            Self::Topology => 1,
        }
    }
}

/// Upper bounds of the request-duration histogram, in seconds.
const DURATION_BUCKETS_SECONDS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// One node label with its availability gauge.
struct NodeGauge {
    label: String,
    available: AtomicU64,
}

/// One fixed-bucket duration histogram, in microseconds, for one operation.
struct Histogram {
    /// Number of observations at or below each bucket bound.
    buckets: [AtomicU64; DURATION_BUCKETS_SECONDS.len()],
    /// Sum of all observations in microseconds.
    sum_micros: AtomicU64,
    /// Number of observations.
    count: AtomicU64,
}

impl Histogram {
    fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            sum_micros: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record one observation of `duration_micros`.
    fn observe(&self, duration_micros: u64) {
        let seconds = duration_micros as f64 / 1_000_000.0;
        for (index, bound) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            if seconds <= *bound {
                self.buckets[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.sum_micros
            .fetch_add(duration_micros, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Metric registry with a fixed label set.
pub struct Metrics {
    nodes: Vec<NodeGauge>,
    quorum_available: AtomicU64,
    histograms: [Histogram; 2],
}

impl Metrics {
    /// Create a registry for `node_labels` with every node and the quorum available.
    pub fn new(node_labels: &[&str]) -> Self {
        Self {
            nodes: node_labels
                .iter()
                .map(|label| NodeGauge {
                    label: (*label).to_owned(),
                    available: AtomicU64::new(1),
                })
                .collect(),
            quorum_available: AtomicU64::new(1),
            histograms: [Histogram::new(), Histogram::new()],
        }
    }

    /// Set the availability gauge of a configured node.
    ///
    /// Returns `false` when `label` is not a configured cluster label, so a
    /// caller cannot widen the label set.
    pub fn set_node_available(&self, label: &str, available: bool) -> bool {
        match self.nodes.iter().find(|node| node.label == label) {
            Some(node) => {
                node.available
                    .store(u64::from(available), Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Set the quorum availability gauge.
    pub fn set_quorum_available(&self, available: bool) {
        self.quorum_available
            .store(u64::from(available), Ordering::Relaxed);
    }

    /// Record one handled request.
    pub fn observe_request(&self, operation: Operation, duration_micros: u64) {
        self.histograms[operation.index()].observe(duration_micros);
    }

    /// Render the Prometheus text exposition for the current values.
    pub fn render(&self, dropped_records: u64) -> String {
        let mut output = String::new();

        write_gauge(
            &mut output,
            METRIC_UP,
            "1 while the dedicated server serves its metrics endpoint.",
            1,
        );
        write_gauge(
            &mut output,
            METRIC_QUORUM_AVAILABLE,
            "1 while a strict majority of the configured storage nodes is available.",
            self.quorum_available.load(Ordering::Relaxed),
        );

        let _ = writeln!(
            output,
            "# HELP {METRIC_NODE_AVAILABLE} 1 while the storage node is available to the dedicated server."
        );
        let _ = writeln!(output, "# TYPE {METRIC_NODE_AVAILABLE} gauge");
        for node in &self.nodes {
            let _ = writeln!(
                output,
                "{METRIC_NODE_AVAILABLE}{{node=\"{}\"}} {}",
                escape_label(&node.label),
                node.available.load(Ordering::Relaxed)
            );
        }

        let _ = writeln!(
            output,
            "# HELP {METRIC_LOG_DROPPED} Log records lost because the writer queue was full or the writer failed."
        );
        let _ = writeln!(output, "# TYPE {METRIC_LOG_DROPPED} counter");
        let _ = writeln!(output, "{METRIC_LOG_DROPPED} {dropped_records}");

        write_gauge(
            &mut output,
            METRIC_LOG_QUEUE_CAPACITY,
            "Capacity of the bounded log writer queue in records.",
            crate::logging::LOG_QUEUE_CAPACITY as u64,
        );

        let _ = writeln!(
            output,
            "# HELP {METRIC_REQUEST_DURATION} Duration of handled client requests in seconds."
        );
        let _ = writeln!(output, "# TYPE {METRIC_REQUEST_DURATION} histogram");
        for operation in Operation::ALL {
            let histogram = &self.histograms[operation.index()];
            let operation_label = operation.label();
            for (index, bound) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
                let _ = writeln!(
                    output,
                    "{METRIC_REQUEST_DURATION}_bucket{{operation=\"{operation_label}\",le=\"{bound}\"}} {}",
                    histogram.buckets[index].load(Ordering::Relaxed)
                );
            }
            let count = histogram.count.load(Ordering::Relaxed);
            let _ = writeln!(
                output,
                "{METRIC_REQUEST_DURATION}_bucket{{operation=\"{operation_label}\",le=\"+Inf\"}} {count}"
            );
            let sum_seconds = histogram.sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
            let _ = writeln!(
                output,
                "{METRIC_REQUEST_DURATION}_sum{{operation=\"{operation_label}\"}} {sum_seconds}"
            );
            let _ = writeln!(
                output,
                "{METRIC_REQUEST_DURATION}_count{{operation=\"{operation_label}\"}} {count}"
            );
        }

        output
    }
}

fn write_gauge(output: &mut String, name: &str, help: &str, value: u64) {
    let _ = writeln!(output, "# HELP {name} {help}");
    let _ = writeln!(output, "# TYPE {name} gauge");
    let _ = writeln!(output, "{name} {value}");
}

/// Escape a label value for the exposition format.
fn escape_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics::new(&["storage-a", "storage-b"])
    }

    #[test]
    fn every_metric_category_is_rendered() {
        let output = metrics().render(7);

        assert!(output.contains("# TYPE homecloud_dedicated_up gauge"));
        assert!(output.contains("\nhomecloud_dedicated_up 1\n"));
        assert!(output.contains("homecloud_dedicated_node_available{node=\"storage-a\"} 1"));
        assert!(output.contains("homecloud_dedicated_node_available{node=\"storage-b\"} 1"));
        assert!(output.contains("\nhomecloud_dedicated_quorum_available 1\n"));
        assert!(output.contains("\nhomecloud_dedicated_log_dropped_records_total 7\n"));
        assert!(output.contains("homecloud_dedicated_log_queue_capacity_records 4096"));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_count{operation=\"registration\"} 0"
        ));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_count{operation=\"topology\"} 0"
        ));
    }

    #[test]
    fn availability_changes_reach_the_gauges() {
        let metrics = metrics();
        assert!(metrics.set_node_available("storage-a", false));
        metrics.set_quorum_available(false);

        let output = metrics.render(0);
        assert!(output.contains("homecloud_dedicated_node_available{node=\"storage-a\"} 0"));
        assert!(output.contains("homecloud_dedicated_node_available{node=\"storage-b\"} 1"));
        assert!(output.contains("\nhomecloud_dedicated_quorum_available 0\n"));
    }

    #[test]
    fn an_unknown_node_label_is_rejected() {
        let metrics = metrics();
        assert!(!metrics.set_node_available("login=alice", false));
        assert!(!metrics.render(0).contains("alice"));
    }

    #[test]
    fn label_values_are_escaped() {
        let metrics = Metrics::new(&["node\"x\\y"]);
        let output = metrics.render(0);
        assert!(output.contains("node=\"node\\\"x\\\\y\""));
    }

    #[test]
    fn observations_fill_the_histogram() {
        let metrics = metrics();
        metrics.observe_request(Operation::Topology, 3_000);
        metrics.observe_request(Operation::Topology, 20_000);

        let output = metrics.render(0);
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_bucket{operation=\"topology\",le=\"0.005\"} 1"
        ));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_bucket{operation=\"topology\",le=\"0.025\"} 2"
        ));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_count{operation=\"topology\"} 2"
        ));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_sum{operation=\"topology\"} 0.023"
        ));
        assert!(output.contains(
            "homecloud_dedicated_request_duration_seconds_count{operation=\"registration\"} 0"
        ));
    }
}
