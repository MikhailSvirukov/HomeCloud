//! Pure, deterministic heartbeat tracking for the fixed storage cluster.
//!
//! The dedicated server sends its own heartbeat exchanges to every storage
//! node, separate from any consensus traffic. This module only models the
//! scheduling and consecutive-miss bookkeeping for those exchanges: it does no
//! I/O, is not async, and keeps availability private, reporting it only through
//! [`HeartbeatTracker::unavailable`]. Callers drive it with explicit [`Instant`]
//! values and carry out the returned [`HeartbeatAction`]s at the system
//! boundary.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use thiserror::Error;

type Result<T> = core::result::Result<T, HeartbeatError>;

/// Availability of a storage node as seen by the dedicated server.
#[derive(Debug, PartialEq, Eq)]
enum Availability {
    Healthy,
    Unavailable,
}

/// Timing and consecutive-miss threshold for heartbeat exchanges.
pub struct HeartbeatTiming {
    /// Interval between heartbeat sends.
    pub interval: Duration,
    /// Time to wait for a response before an attempt times out.
    pub timeout: Duration,
    /// Consecutive misses tolerated before a node is considered unavailable.
    pub miss_threshold: NonZeroU32,
}

/// A heartbeat action the runtime must carry out for a node.
#[derive(Debug, PartialEq, Eq)]
pub enum HeartbeatAction {
    /// Send a heartbeat now; the attempt stays in flight until a response or the timeout.
    Send { label: String },
    /// The in-flight attempt exceeded the timeout and counts as a miss.
    TimedOut { label: String },
}

/// Heartbeat state kept for one storage node.
struct NodeState {
    label: String,
    consecutive_misses: u32,
    availability: Availability,
    next_attempt_at: Instant,
    in_flight_deadline: Option<Instant>,
}

/// Errors produced by heartbeat tracking.
#[derive(Debug, Error)]
pub enum HeartbeatError {
    /// The requested label is not part of the cluster.
    #[error("unknown cluster node '{0}'")]
    UnknownNode(String),
    /// A response was recorded while no heartbeat attempt was in flight.
    #[error("no heartbeat attempt is in flight for node '{0}'")]
    NoAttemptInFlight(String),
    /// A cluster node label was empty or repeated.
    #[error("cluster node labels must be non-empty and unique, but got '{label}'")]
    InvalidNodeLabel { label: String },
}

/// Tracks heartbeat scheduling and consecutive misses for the fixed cluster.
pub struct HeartbeatTracker {
    interval: Duration,
    timeout: Duration,
    miss_threshold: NonZeroU32,
    nodes: Vec<NodeState>,
    index: HashMap<String, usize>,
}

impl HeartbeatTracker {
    /// Create a tracker with every node healthy, idle, and due at `start`,
    /// rejecting empty or repeated cluster node labels.
    pub fn new(labels: &[&str], timing: HeartbeatTiming, start: Instant) -> Result<Self> {
        let mut seen = HashSet::with_capacity(labels.len());

        for label in labels {
            if label.is_empty() || !seen.insert(*label) {
                return Err(HeartbeatError::InvalidNodeLabel {
                    label: (*label).to_string(),
                });
            }
        }

        let nodes = labels
            .iter()
            .map(|label| NodeState {
                label: label.to_string(),
                consecutive_misses: 0,
                availability: Availability::Healthy,
                next_attempt_at: start,
                in_flight_deadline: None,
            })
            .collect::<Vec<_>>();

        let index = nodes
            .iter()
            .enumerate()
            .map(|(position, node)| (node.label.clone(), position))
            .collect();

        Ok(Self {
            interval: timing.interval,
            timeout: timing.timeout,
            miss_threshold: timing.miss_threshold,
            nodes,
            index,
        })
    }

    /// Collect and apply everything due at `now`, in configured cluster order.
    ///
    /// Each node contributes at most one action: an in-flight attempt at or
    /// past its deadline produces [`HeartbeatAction::TimedOut`], and a node
    /// with nothing in flight at or past `next_attempt_at` produces
    /// [`HeartbeatAction::Send`]. A node that timed out is not sent to in the
    /// same call. Nodes that are already unavailable are still polled so a
    /// later response can recover them.
    pub fn poll(&mut self, now: Instant) -> Vec<HeartbeatAction> {
        let mut actions = Vec::new();

        for state in &mut self.nodes {
            let action = match state.in_flight_deadline {
                Some(deadline) if now >= deadline => {
                    state.consecutive_misses += 1;
                    if state.consecutive_misses >= self.miss_threshold.get() {
                        state.availability = Availability::Unavailable;
                    }
                    state.in_flight_deadline = None;
                    state.next_attempt_at = now + self.interval;
                    Some(HeartbeatAction::TimedOut {
                        label: state.label.clone(),
                    })
                }
                None if now >= state.next_attempt_at => {
                    state.in_flight_deadline = Some(now + self.timeout);
                    Some(HeartbeatAction::Send {
                        label: state.label.clone(),
                    })
                }
                _ => None,
            };

            if let Some(action) = action {
                actions.push(action);
            }
        }

        actions
    }

    /// Record a response, clearing the miss counter and rescheduling from the response time.
    pub fn record_response(&mut self, label: &str, at: Instant) -> Result<()> {
        let position = self.position_of(label)?;
        let state = &mut self.nodes[position];

        if state.in_flight_deadline.is_none() {
            return Err(HeartbeatError::NoAttemptInFlight(label.to_string()));
        }

        state.consecutive_misses = 0;
        state.availability = Availability::Healthy;
        state.in_flight_deadline = None;
        state.next_attempt_at = at + self.interval;

        Ok(())
    }

    /// Labels of nodes currently considered unavailable, in configured cluster order.
    pub fn unavailable(&self) -> Vec<&str> {
        self.nodes
            .iter()
            .filter(|node| node.availability == Availability::Unavailable)
            .map(|node| node.label.as_str())
            .collect()
    }

    fn position_of(&self, label: &str) -> Result<usize> {
        self.index
            .get(label)
            .copied()
            .ok_or_else(|| HeartbeatError::UnknownNode(label.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const LABELS: [&str; 2] = ["storage-a", "storage-b"];
    const UNKNOWN: &str = "storage-unknown";

    const INTERVAL: Duration = Duration::from_millis(1000);
    const TIMEOUT: Duration = Duration::from_millis(400);
    const MISS_THRESHOLD: u32 = 2;
    const TIMING: HeartbeatTiming = HeartbeatTiming {
        interval: INTERVAL,
        timeout: TIMEOUT,
        miss_threshold: NonZeroU32::new(MISS_THRESHOLD).expect("MISS_THRESHOLD is non-zero"),
    };

    #[test]
    fn due_nodes_are_sent_to_in_configured_order() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS, TIMING, start)
            .expect("the test cluster labels are non-empty and unique");

        let actions = tracker.poll(start);

        assert_eq!(
            actions,
            vec![
                HeartbeatAction::Send {
                    label: LABELS[0].to_string(),
                },
                HeartbeatAction::Send {
                    label: LABELS[1].to_string(),
                },
            ]
        );
    }

    #[test]
    fn nothing_is_emitted_before_the_next_attempt_is_due() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS, TIMING, start)
            .expect("the test cluster labels are non-empty and unique");
        tracker.poll(start);
        tracker.poll(start + TIMEOUT);

        let actions = tracker.poll(start + TIMEOUT + INTERVAL - Duration::from_millis(1));

        assert!(actions.is_empty());
    }

    #[test]
    fn an_expired_in_flight_attempt_times_out_without_a_send() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS[..1], TIMING, start)
            .expect("the test cluster labels are non-empty and unique");
        tracker.poll(start);

        let actions = tracker.poll(start + TIMEOUT);

        assert_eq!(
            actions,
            vec![HeartbeatAction::TimedOut {
                label: LABELS[0].to_string(),
            }]
        );
    }

    #[test]
    fn a_single_miss_keeps_the_node_healthy() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS[..1], TIMING, start)
            .expect("the test cluster labels are non-empty and unique");

        tracker.poll(start);
        tracker.poll(start + TIMEOUT);

        assert_eq!(tracker.nodes[0].consecutive_misses, 1);
        assert_eq!(tracker.nodes[0].availability, Availability::Healthy);
        assert_eq!(tracker.nodes[0].in_flight_deadline, None);
        assert_eq!(tracker.nodes[0].next_attempt_at, start + TIMEOUT + INTERVAL);
        assert!(tracker.unavailable().is_empty());
    }

    #[test]
    fn reaching_the_miss_threshold_marks_the_node_unavailable() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS[..1], TIMING, start)
            .expect("the test cluster labels are non-empty and unique");

        tracker.poll(start);
        tracker.poll(start + TIMEOUT);
        tracker.poll(start + TIMEOUT + INTERVAL);
        tracker.poll(start + TIMEOUT + INTERVAL + TIMEOUT);

        assert_eq!(tracker.nodes[0].consecutive_misses, MISS_THRESHOLD);
        assert_eq!(tracker.nodes[0].availability, Availability::Unavailable);
        assert_eq!(tracker.unavailable(), vec![LABELS[0]]);
    }

    #[test]
    fn a_response_resets_the_miss_counter_and_reschedules_from_the_response_time() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS[..1], TIMING, start)
            .expect("the test cluster labels are non-empty and unique");
        tracker.poll(start);
        tracker.poll(start + TIMEOUT);
        tracker.poll(start + TIMEOUT + INTERVAL);
        let response_at = start + TIMEOUT + INTERVAL + Duration::from_millis(100);

        tracker
            .record_response(LABELS[0], response_at)
            .expect("an attempt is in flight");

        assert_eq!(tracker.nodes[0].consecutive_misses, 0);
        assert_eq!(tracker.nodes[0].in_flight_deadline, None);
        assert_eq!(tracker.nodes[0].next_attempt_at, response_at + INTERVAL);
        assert_eq!(
            tracker.poll(response_at + INTERVAL),
            vec![HeartbeatAction::Send {
                label: LABELS[0].to_string(),
            }]
        );
    }

    #[test]
    fn an_unavailable_node_recovers_to_healthy_through_a_response() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS[..1], TIMING, start)
            .expect("the test cluster labels are non-empty and unique");
        tracker.poll(start);
        tracker.poll(start + TIMEOUT);
        tracker.poll(start + TIMEOUT + INTERVAL);
        let marked_at = start + TIMEOUT + INTERVAL + TIMEOUT;
        tracker.poll(marked_at);
        assert_eq!(tracker.nodes[0].availability, Availability::Unavailable);

        tracker.poll(marked_at + INTERVAL);
        let response_at = marked_at + INTERVAL + Duration::from_millis(100);
        tracker
            .record_response(LABELS[0], response_at)
            .expect("an attempt is in flight");

        assert_eq!(tracker.nodes[0].availability, Availability::Healthy);
        assert_eq!(tracker.nodes[0].consecutive_misses, 0);
        assert!(tracker.unavailable().is_empty());
    }

    #[test]
    fn recording_a_response_for_an_unknown_label_is_an_error() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS, TIMING, start)
            .expect("the test cluster labels are non-empty and unique");

        let error = tracker
            .record_response(UNKNOWN, start)
            .expect_err("an unknown label must fail");

        assert!(matches!(error, HeartbeatError::UnknownNode(label) if label == UNKNOWN));
    }

    #[test]
    fn recording_a_response_with_nothing_in_flight_is_an_error() {
        let start = Instant::now();
        let mut tracker = HeartbeatTracker::new(&LABELS, TIMING, start)
            .expect("the test cluster labels are non-empty and unique");

        let error = tracker
            .record_response(LABELS[0], start)
            .expect_err("no attempt is in flight");

        assert!(matches!(error, HeartbeatError::NoAttemptInFlight(label) if label == LABELS[0]));
    }

    #[test]
    fn empty_and_repeated_labels_are_rejected() {
        let start = Instant::now();

        let empty = HeartbeatTracker::new(&[""], TIMING, start)
            .err()
            .expect("an empty label must be rejected");
        assert!(matches!(empty, HeartbeatError::InvalidNodeLabel { label } if label.is_empty()));

        let repeated = HeartbeatTracker::new(&[LABELS[0], LABELS[0]], TIMING, start)
            .err()
            .expect("a repeated label must be rejected");
        assert!(matches!(
            repeated,
            HeartbeatError::InvalidNodeLabel { label } if label == LABELS[0]
        ));
    }
}
