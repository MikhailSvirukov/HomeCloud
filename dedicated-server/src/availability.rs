//! Availability observation for the fixed storage cluster.
//!
//! [`AvailabilityMonitor`] owns the pure [`HeartbeatTracker`] and reports the
//! significant changes `NFR-OBS-002` requires to be logged: a storage node
//! becoming unavailable or available again, and the cluster losing or
//! regaining a quorum. A successful periodic heartbeat is not an event; only a
//! transition or a heartbeat failure is reported, and each transition is
//! reported once, in the configured cluster order.

use std::sync::Arc;
use std::time::Instant;

use crate::heartbeat::{HeartbeatAction, HeartbeatError, HeartbeatTiming, HeartbeatTracker};
use crate::logging;
use crate::metrics::Metrics;

/// A significant availability change observed by [`AvailabilityMonitor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvailabilityEvent {
    /// A storage node stopped responding to heartbeats.
    NodeUnavailable {
        /// Configured cluster label of the node.
        label: String,
    },
    /// A storage node that was unavailable responded again.
    NodeAvailable {
        /// Configured cluster label of the node.
        label: String,
    },
    /// The available nodes stopped forming a strict majority.
    QuorumLost {
        /// Number of available nodes.
        available: usize,
        /// Number of configured nodes.
        total: usize,
    },
    /// The available nodes form a strict majority again.
    QuorumRestored {
        /// Number of available nodes.
        available: usize,
        /// Number of configured nodes.
        total: usize,
    },
}

/// Turns heartbeat state into the availability transitions worth logging.
pub struct AvailabilityMonitor {
    labels: Vec<String>,
    tracker: HeartbeatTracker,
    unavailable: Vec<String>,
    quorum_available: bool,
}

impl AvailabilityMonitor {
    /// Create a monitor over `labels` with every node healthy and idle.
    ///
    /// Fails when a label is empty or repeated, exactly like
    /// [`HeartbeatTracker::new`].
    pub fn new(
        labels: &[&str],
        timing: HeartbeatTiming,
        start: Instant,
    ) -> Result<Self, HeartbeatError> {
        let tracker = HeartbeatTracker::new(labels, timing, start)?;
        let total = labels.len();
        let labels = labels
            .iter()
            .map(|label| (*label).to_owned())
            .collect::<Vec<_>>();

        Ok(Self {
            labels,
            tracker,
            unavailable: Vec::new(),
            quorum_available: quorum_reached(total, total),
        })
    }

    /// Collect the heartbeat actions due at `now` and the transitions they caused.
    pub fn poll(&mut self, now: Instant) -> (Vec<HeartbeatAction>, Vec<AvailabilityEvent>) {
        let actions = self.tracker.poll(now);
        (actions, self.transitions())
    }

    /// Record a heartbeat response and return the transitions it caused.
    ///
    /// A rejected response is logged as a heartbeat failure and returned to
    /// the caller; a successful response on a healthy node yields an empty
    /// list and produces no record.
    pub fn record_response(
        &mut self,
        label: &str,
        at: Instant,
    ) -> Result<Vec<AvailabilityEvent>, HeartbeatError> {
        match self.tracker.record_response(label, at) {
            Ok(()) => Ok(self.transitions()),
            Err(error) => {
                logging::heartbeat_error(&error);
                Err(error)
            }
        }
    }

    /// Labels currently considered unavailable, in configured cluster order.
    pub fn unavailable(&self) -> &[String] {
        &self.unavailable
    }

    /// Configured cluster labels, in manifest order.
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Whether the available nodes currently form a strict majority.
    pub fn quorum_available(&self) -> bool {
        self.quorum_available
    }

    fn transitions(&mut self) -> Vec<AvailabilityEvent> {
        let current = self
            .tracker
            .unavailable()
            .into_iter()
            .map(|label| label.to_owned())
            .collect::<Vec<_>>();

        let mut events = Vec::new();
        for label in &self.labels {
            let was_unavailable = self.unavailable.iter().any(|previous| previous == label);
            let is_unavailable = current.iter().any(|node| node == label);
            match (was_unavailable, is_unavailable) {
                (false, true) => events.push(AvailabilityEvent::NodeUnavailable {
                    label: label.clone(),
                }),
                (true, false) => events.push(AvailabilityEvent::NodeAvailable {
                    label: label.clone(),
                }),
                _ => {}
            }
        }

        let total = self.labels.len();
        let available = total.saturating_sub(current.len());
        let quorum_available = quorum_reached(available, total);
        if quorum_available != self.quorum_available {
            events.push(if quorum_available {
                AvailabilityEvent::QuorumRestored { available, total }
            } else {
                AvailabilityEvent::QuorumLost { available, total }
            });
            self.quorum_available = quorum_available;
        }

        self.unavailable = current;
        events
    }
}

/// Drives the monitor from the heartbeat loop and publishes its state.
///
/// The registry is shared with the metrics endpoint, so every poll and every
/// recorded response is visible in the next scrape.
pub struct AvailabilityPublisher {
    monitor: AvailabilityMonitor,
    metrics: Arc<Metrics>,
}

impl AvailabilityPublisher {
    /// Publish the initial state and return the publisher.
    pub fn new(monitor: AvailabilityMonitor, metrics: Arc<Metrics>) -> Self {
        let publisher = Self { monitor, metrics };
        publisher.publish();
        publisher
    }

    /// Collect the due heartbeat actions, log the transitions and publish them.
    pub fn poll(&mut self, now: Instant) -> Vec<HeartbeatAction> {
        let (actions, events) = self.monitor.poll(now);
        log_events(&events);
        self.publish();
        actions
    }

    /// Record a heartbeat response, log the transitions and publish them.
    ///
    /// A rejected response is logged and returned unchanged, exactly as
    /// [`AvailabilityMonitor::record_response`] reports it.
    pub fn record_response(&mut self, label: &str, at: Instant) -> Result<(), HeartbeatError> {
        let events = self.monitor.record_response(label, at)?;
        log_events(&events);
        self.publish();
        Ok(())
    }

    /// Configured cluster labels, in manifest order.
    pub fn labels(&self) -> &[String] {
        self.monitor.labels()
    }

    fn publish(&self) {
        let unavailable = self.monitor.unavailable();
        for label in self.monitor.labels() {
            self.metrics
                .set_node_available(label, !unavailable.iter().any(|node| node == label));
        }
        self.metrics
            .set_quorum_available(self.monitor.quorum_available());
    }
}

/// Emit one structured log record per availability event.
pub fn log_events(events: &[AvailabilityEvent]) {
    for event in events {
        match event {
            AvailabilityEvent::NodeUnavailable { label } => {
                logging::node_availability(label, false);
            }
            AvailabilityEvent::NodeAvailable { label } => {
                logging::node_availability(label, true);
            }
            AvailabilityEvent::QuorumLost { available, total } => {
                logging::quorum_availability(false, *available, *total);
            }
            AvailabilityEvent::QuorumRestored { available, total } => {
                logging::quorum_availability(true, *available, *total);
            }
        }
    }
}

/// A strict majority of the configured nodes is required for a quorum.
fn quorum_reached(available: usize, total: usize) -> bool {
    available.saturating_mul(2) > total
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::{Duration, Instant};

    use super::*;

    const LABELS: [&str; 3] = ["storage-a", "storage-b", "storage-c"];
    const INTERVAL_MS: u64 = 1_000;
    const TIMEOUT_MS: u64 = 10;

    fn timing() -> HeartbeatTiming {
        HeartbeatTiming {
            interval_ms: NonZeroU64::new(INTERVAL_MS).expect("interval must be non-zero"),
            timeout_ms: NonZeroU64::new(TIMEOUT_MS).expect("timeout must be non-zero"),
            miss_threshold: NonZeroU32::new(2).expect("threshold must be non-zero"),
        }
    }

    fn monitor() -> (AvailabilityMonitor, Instant) {
        let start = Instant::now();
        let monitor =
            AvailabilityMonitor::new(&LABELS, timing(), start).expect("labels must be valid");
        (monitor, start)
    }

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn publisher() -> (AvailabilityPublisher, Instant, Arc<Metrics>) {
        let (monitor, start) = monitor();
        let metrics = Arc::new(Metrics::new(&LABELS));
        let publisher = AvailabilityPublisher::new(monitor, Arc::clone(&metrics));
        (publisher, start, metrics)
    }

    fn mark_unavailable(publisher: &mut AvailabilityPublisher, start: Instant, keep: &[usize]) {
        publisher.poll(start);
        for index in keep {
            publisher
                .record_response(LABELS[*index], at(start, 1))
                .expect("the node must have an attempt in flight");
        }
        publisher.poll(at(start, TIMEOUT_MS));
        publisher.poll(at(start, INTERVAL_MS + TIMEOUT_MS));
        publisher.poll(at(start, INTERVAL_MS + TIMEOUT_MS * 2));
    }

    #[test]
    fn an_unavailable_node_is_published_as_a_zero_gauge() {
        let (mut publisher, start, metrics) = publisher();
        mark_unavailable(&mut publisher, start, &[1, 2]);

        let rendered = metrics.render(0);
        assert!(
            rendered.contains("homecloud_dedicated_node_available{node=\"storage-a\"} 0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("homecloud_dedicated_quorum_available 1"),
            "{rendered}"
        );
    }

    #[test]
    fn losing_the_quorum_is_published() {
        let (mut publisher, start, metrics) = publisher();
        mark_unavailable(&mut publisher, start, &[0]);

        let rendered = metrics.render(0);
        assert!(
            rendered.contains("homecloud_dedicated_node_available{node=\"storage-b\"} 0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("homecloud_dedicated_quorum_available 0"),
            "{rendered}"
        );
    }

    #[test]
    fn a_response_restores_the_node_and_the_quorum() {
        let (mut publisher, start, metrics) = publisher();
        mark_unavailable(&mut publisher, start, &[0]);
        assert!(
            metrics
                .render(0)
                .contains("homecloud_dedicated_quorum_available 0"),
            "the quorum must be lost before the recovery"
        );

        publisher
            .record_response(LABELS[0], at(start, INTERVAL_MS + TIMEOUT_MS * 2))
            .expect("the node must have an attempt in flight");

        let rendered = metrics.render(0);
        assert!(
            rendered.contains("homecloud_dedicated_node_available{node=\"storage-a\"} 1"),
            "{rendered}"
        );
        assert!(
            rendered.contains("homecloud_dedicated_quorum_available 1"),
            "{rendered}"
        );
    }

    #[test]
    fn a_node_crossing_the_miss_threshold_is_reported_once() {
        let (mut monitor, start) = monitor();

        let (actions, events) = monitor.poll(start);
        assert_eq!(actions.len(), LABELS.len());
        assert!(events.is_empty(), "a healthy start is not a transition");
        assert!(
            monitor
                .record_response(LABELS[1], at(start, 1))
                .expect("the node must have an attempt in flight")
                .is_empty(),
            "a successful heartbeat is not an event"
        );
        assert!(monitor
            .record_response(LABELS[2], at(start, 1))
            .expect("the node must have an attempt in flight")
            .is_empty());

        assert!(
            monitor.poll(at(start, TIMEOUT_MS)).1.is_empty(),
            "the first miss is not an event"
        );
        monitor.poll(at(start, INTERVAL_MS + TIMEOUT_MS));
        let (_, events) = monitor.poll(at(start, INTERVAL_MS + TIMEOUT_MS * 2));

        assert_eq!(
            events,
            vec![AvailabilityEvent::NodeUnavailable {
                label: LABELS[0].to_owned()
            }],
            "losing one of three nodes keeps the quorum, so only the node transition is reported"
        );
        assert_eq!(monitor.unavailable(), &[LABELS[0].to_owned()]);

        let (_, events) = monitor.poll(at(start, INTERVAL_MS * 2 + TIMEOUT_MS * 2));
        assert!(
            events.is_empty(),
            "an unavailable node reports no new event"
        );

        let recovered = monitor
            .record_response(LABELS[0], at(start, INTERVAL_MS * 2 + TIMEOUT_MS * 2))
            .expect("the node must have an attempt in flight");
        assert_eq!(
            recovered,
            vec![AvailabilityEvent::NodeAvailable {
                label: LABELS[0].to_owned()
            }]
        );
        assert!(monitor.unavailable().is_empty());
    }

    #[test]
    fn quorum_loss_and_restoration_are_reported_once_in_configured_order() {
        let (mut monitor, start) = monitor();

        monitor.poll(start);
        monitor.poll(at(start, TIMEOUT_MS));
        monitor.poll(at(start, INTERVAL_MS + TIMEOUT_MS));
        let (_, events) = monitor.poll(at(start, INTERVAL_MS + TIMEOUT_MS * 2));

        assert_eq!(
            events,
            vec![
                AvailabilityEvent::NodeUnavailable {
                    label: LABELS[0].to_owned()
                },
                AvailabilityEvent::NodeUnavailable {
                    label: LABELS[1].to_owned()
                },
                AvailabilityEvent::NodeUnavailable {
                    label: LABELS[2].to_owned()
                },
                AvailabilityEvent::QuorumLost {
                    available: 0,
                    total: LABELS.len()
                },
            ],
            "the transitions follow the configured order and the quorum is reported once"
        );

        let (_, events) = monitor.poll(at(start, INTERVAL_MS * 2 + TIMEOUT_MS * 2));
        assert!(
            events.is_empty(),
            "a poll without a transition reports nothing"
        );

        let recovered = monitor
            .record_response(LABELS[1], at(start, INTERVAL_MS * 2 + TIMEOUT_MS * 2))
            .expect("the node must have an attempt in flight");
        assert_eq!(
            recovered,
            vec![AvailabilityEvent::NodeAvailable {
                label: LABELS[1].to_owned()
            }],
            "one of three nodes is still not a quorum"
        );

        let restored = monitor
            .record_response(LABELS[2], at(start, INTERVAL_MS * 2 + TIMEOUT_MS * 2))
            .expect("the node must have an attempt in flight");
        assert_eq!(
            restored,
            vec![
                AvailabilityEvent::NodeAvailable {
                    label: LABELS[2].to_owned()
                },
                AvailabilityEvent::QuorumRestored {
                    available: 2,
                    total: LABELS.len()
                },
            ]
        );

        let (_, events) = monitor.poll(at(start, INTERVAL_MS * 4));
        assert!(events.is_empty(), "no transition is reported twice");
    }

    #[test]
    fn a_rejected_response_is_returned_as_an_error() {
        let (mut monitor, start) = monitor();
        let error = monitor
            .record_response(LABELS[0], start)
            .expect_err("no attempt is in flight before the first poll");
        assert!(matches!(error, HeartbeatError::NoAttemptInFlight(_)));
        assert!(monitor.record_response("storage-unknown", start).is_err());
    }

    #[test]
    fn repeated_labels_are_rejected() {
        let result = AvailabilityMonitor::new(&["node", "node"], timing(), Instant::now());
        assert!(matches!(
            result,
            Err(HeartbeatError::InvalidNodeLabel { .. })
        ));
    }

    #[test]
    fn quorum_requires_a_strict_majority() {
        assert!(!quorum_reached(1, 2));
        assert!(quorum_reached(2, 3));
        assert!(!quorum_reached(0, 0));
    }
}
