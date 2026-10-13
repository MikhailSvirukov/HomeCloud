//! Pure, deterministic heartbeat tracking for the fixed storage cluster.
//!
//! The dedicated server sends its own heartbeat exchanges to every storage
//! node, separate from any consensus traffic. This module only models the
//! scheduling and consecutive-miss bookkeeping for those exchanges: it does no
//! I/O, is not async, and never exposes availability outside the dedicated
//! server. Callers drive it with explicit [`Instant`] values and carry out the
//! returned [`HeartbeatAction`]s at the system boundary.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::config::{ClusterConfig, HeartbeatConfig};

/// Internal availability of a storage node as seen by the dedicated server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// The node answered within the configured timeout.
    Healthy,
    /// The node reached the configured number of consecutive misses.
    Unavailable,
}

/// A heartbeat action the runtime must carry out for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatAction {
    /// Send a heartbeat now; the attempt stays in flight until a response or the timeout.
    Send { label: String },
    /// The in-flight attempt exceeded the configured timeout and counts as a miss.
    TimedOut { label: String },
}

/// Internal heartbeat state kept for one storage node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHeartbeatState {
    /// Label identifying the node in the configured cluster.
    pub label: String,
    /// Number of consecutive missed heartbeats since the last response.
    pub consecutive_misses: u32,
    /// Availability derived from [`Self::consecutive_misses`].
    pub availability: Availability,
    /// Earliest time the next heartbeat may be sent.
    pub next_attempt_at: Instant,
    /// Deadline of the in-flight attempt, or `None` when nothing is in flight.
    pub in_flight_deadline: Option<Instant>,
}

/// Errors produced while recording heartbeat results.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HeartbeatError {
    /// The requested label is not part of the configured cluster.
    #[error("unknown cluster node '{0}'")]
    UnknownNode(String),
    /// A response was recorded while no heartbeat attempt was in flight.
    #[error("no heartbeat attempt is in flight for node '{0}'")]
    NoAttemptInFlight(String),
}

/// Tracks heartbeat scheduling and consecutive misses for the fixed cluster.
///
/// Node order follows [`ClusterConfig::nodes`]; a label-keyed index provides
/// lookups without rescanning the node order.
#[derive(Debug)]
pub struct HeartbeatTracker {
    interval: Duration,
    timeout: Duration,
    miss_threshold: u32,
    nodes: Vec<NodeHeartbeatState>,
    index: HashMap<String, usize>,
}

impl HeartbeatTracker {
    /// Create a tracker for `cluster`; every node becomes due at `start`.
    ///
    /// Each node starts healthy with no attempt in flight and
    /// `next_attempt_at = start`. Timing comes from `heartbeat`.
    pub fn new(cluster: &ClusterConfig, heartbeat: &HeartbeatConfig, start: Instant) -> Self {
        let nodes = cluster
            .nodes
            .iter()
            .map(|node| NodeHeartbeatState {
                label: node.label.clone(),
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

        Self {
            interval: heartbeat.interval(),
            timeout: heartbeat.timeout(),
            miss_threshold: heartbeat.miss_threshold.get(),
            nodes,
            index,
        }
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
                    if state.consecutive_misses >= self.miss_threshold {
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

    /// Record a response for `label`, resetting its consecutive miss counter.
    ///
    /// The node becomes healthy again, the in-flight attempt is cleared, and
    /// the next attempt is scheduled one interval after `at`.
    ///
    /// # Errors
    ///
    /// Returns [`HeartbeatError::UnknownNode`] when `label` is not part of the
    /// configured cluster, and [`HeartbeatError::NoAttemptInFlight`] when no
    /// heartbeat attempt is currently in flight for the node.
    pub fn record_response(&mut self, label: &str, at: Instant) -> Result<(), HeartbeatError> {
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

    /// Internal state of one node.
    ///
    /// # Errors
    ///
    /// Returns [`HeartbeatError::UnknownNode`] when `label` is not part of the
    /// configured cluster.
    pub fn node_state(&self, label: &str) -> Result<&NodeHeartbeatState, HeartbeatError> {
        let position = self.position_of(label)?;
        Ok(&self.nodes[position])
    }

    /// Internal state of every node, in configured cluster order.
    pub fn states(&self) -> Vec<&NodeHeartbeatState> {
        self.nodes.iter().collect()
    }

    /// Labels of nodes currently considered unavailable, in configured cluster order.
    pub fn unavailable(&self) -> Vec<&str> {
        self.nodes
            .iter()
            .filter(|node| node.availability == Availability::Unavailable)
            .map(|node| node.label.as_str())
            .collect()
    }

    /// Resolve a node label to its position in configured cluster order.
    fn position_of(&self, label: &str) -> Result<usize, HeartbeatError> {
        self.index
            .get(label)
            .copied()
            .ok_or_else(|| HeartbeatError::UnknownNode(label.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use super::*;
    use crate::config::ClusterNode;

    const LABEL_A: &str = "storage-a";
    const LABEL_B: &str = "storage-b";

    const INTERVAL_MS: u64 = 1000;
    const TIMEOUT_MS: u64 = 400;
    const MISS_THRESHOLD: u32 = 2;

    fn node(label: &str, address: &str) -> ClusterNode {
        ClusterNode {
            label: label.to_string(),
            address: address.parse().expect("test address must parse"),
            encryption_public_key: format!("{label}-encryption"),
            signing_public_key: format!("{label}-signing"),
        }
    }

    fn cluster_of(labels: &[&str]) -> ClusterConfig {
        ClusterConfig {
            nodes: labels
                .iter()
                .enumerate()
                .map(|(position, label)| node(label, &format!("10.0.0.{}:7000", position + 1)))
                .collect(),
        }
    }

    fn heartbeat() -> HeartbeatConfig {
        HeartbeatConfig {
            interval_ms: NonZeroU64::new(INTERVAL_MS).expect("INTERVAL_MS is non-zero"),
            timeout_ms: NonZeroU64::new(TIMEOUT_MS).expect("TIMEOUT_MS is non-zero"),
            miss_threshold: NonZeroU32::new(MISS_THRESHOLD).expect("MISS_THRESHOLD is non-zero"),
        }
    }

    fn cluster() -> ClusterConfig {
        cluster_of(&[LABEL_A, LABEL_B])
    }

    fn tracker(start: Instant) -> HeartbeatTracker {
        HeartbeatTracker::new(&cluster(), &heartbeat(), start)
    }

    fn single_node_tracker(start: Instant) -> HeartbeatTracker {
        HeartbeatTracker::new(&cluster_of(&[LABEL_A]), &heartbeat(), start)
    }

    fn interval() -> Duration {
        Duration::from_millis(INTERVAL_MS)
    }

    fn timeout() -> Duration {
        Duration::from_millis(TIMEOUT_MS)
    }

    fn state_of<'a>(tracker: &'a HeartbeatTracker, label: &str) -> &'a NodeHeartbeatState {
        tracker
            .node_state(label)
            .expect("node must be part of the configured cluster")
    }

    #[test]
    fn new_marks_every_node_due_immediately_and_healthy() {
        let base = Instant::now();
        let tracker = tracker(base);

        for label in [LABEL_A, LABEL_B] {
            let state = state_of(&tracker, label);
            assert_eq!(state.consecutive_misses, 0);
            assert_eq!(state.availability, Availability::Healthy);
            assert_eq!(state.next_attempt_at, base);
            assert_eq!(state.in_flight_deadline, None);
        }
    }

    #[test]
    fn poll_sends_to_due_nodes_in_configured_order() {
        let base = Instant::now();
        let mut tracker = tracker(base);

        let actions = tracker.poll(base);

        assert_eq!(
            actions,
            vec![
                HeartbeatAction::Send {
                    label: LABEL_A.to_string()
                },
                HeartbeatAction::Send {
                    label: LABEL_B.to_string()
                },
            ]
        );
    }

    #[test]
    fn poll_sends_only_to_nodes_that_are_due() {
        let base = Instant::now();
        let mut tracker = tracker(base);

        tracker.poll(base);
        tracker
            .record_response(LABEL_A, base + Duration::from_millis(100))
            .expect("A has an in-flight attempt");
        tracker
            .record_response(LABEL_B, base + Duration::from_millis(300))
            .expect("B has an in-flight attempt");

        let actions = tracker.poll(base + Duration::from_millis(1100));

        assert_eq!(
            actions,
            vec![HeartbeatAction::Send {
                label: LABEL_A.to_string()
            }]
        );
    }

    #[test]
    fn poll_emits_nothing_before_next_attempt_is_due() {
        let base = Instant::now();
        let mut tracker = tracker(base + Duration::from_millis(100));

        let actions = tracker.poll(base);

        assert!(actions.is_empty());
    }

    #[test]
    fn record_response_resets_state_and_schedules_from_response_time() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());
        tracker.poll(base + timeout() + interval());
        let response_at = base + timeout() + interval() + Duration::from_millis(200);

        tracker
            .record_response(LABEL_A, response_at)
            .expect("A has an in-flight attempt");

        let state = state_of(&tracker, LABEL_A);
        assert_eq!(state.consecutive_misses, 0);
        assert_eq!(state.availability, Availability::Healthy);
        assert_eq!(state.in_flight_deadline, None);
        assert_eq!(state.next_attempt_at, response_at + interval());
    }

    #[test]
    fn single_miss_keeps_node_healthy() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());

        let state = state_of(&tracker, LABEL_A);
        assert_eq!(state.consecutive_misses, 1);
        assert_eq!(state.availability, Availability::Healthy);
        assert_eq!(state.in_flight_deadline, None);
        assert_eq!(state.next_attempt_at, base + timeout() + interval());
    }

    #[test]
    fn reaching_miss_threshold_marks_node_unavailable() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());
        tracker.poll(base + timeout() + interval());

        let timed_out_at = base + timeout() + interval() + timeout();
        tracker.poll(timed_out_at);

        let state = state_of(&tracker, LABEL_A);
        assert_eq!(state.consecutive_misses, MISS_THRESHOLD);
        assert_eq!(state.availability, Availability::Unavailable);
    }

    #[test]
    fn response_between_misses_resets_consecutive_counter() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());
        tracker.poll(base + timeout() + interval());
        tracker
            .record_response(
                LABEL_A,
                base + timeout() + interval() + Duration::from_millis(100),
            )
            .expect("A has an in-flight attempt");

        let next_send_at = base + timeout() + interval() + Duration::from_millis(100) + interval();
        tracker.poll(next_send_at);
        tracker.poll(next_send_at + timeout());

        let state = state_of(&tracker, LABEL_A);
        assert_eq!(state.consecutive_misses, 1);
        assert_eq!(state.availability, Availability::Healthy);
    }

    #[test]
    fn expired_in_flight_emits_timeout_without_send() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        assert_eq!(
            tracker.poll(base),
            vec![HeartbeatAction::Send {
                label: LABEL_A.to_string()
            }]
        );

        let actions = tracker.poll(base + timeout());

        assert_eq!(
            actions,
            vec![HeartbeatAction::TimedOut {
                label: LABEL_A.to_string()
            }]
        );
    }

    #[test]
    fn unavailable_node_recovers_to_healthy_after_response() {
        let base = Instant::now();
        let mut tracker = single_node_tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());
        tracker.poll(base + timeout() + interval());
        let marked_at = base + timeout() + interval() + timeout();
        tracker.poll(marked_at);
        assert_eq!(
            state_of(&tracker, LABEL_A).availability,
            Availability::Unavailable
        );

        tracker.poll(marked_at + interval());
        tracker
            .record_response(LABEL_A, marked_at + interval() + Duration::from_millis(100))
            .expect("A has an in-flight attempt");

        let state = state_of(&tracker, LABEL_A);
        assert_eq!(state.availability, Availability::Healthy);
        assert_eq!(state.consecutive_misses, 0);
    }

    #[test]
    fn record_response_on_unknown_label_errors() {
        let base = Instant::now();
        let mut tracker = tracker(base);

        let error = tracker
            .record_response("missing", base)
            .expect_err("unknown label must fail");

        assert_eq!(error, HeartbeatError::UnknownNode("missing".to_string()));
    }

    #[test]
    fn record_response_without_in_flight_attempt_errors() {
        let base = Instant::now();
        let mut tracker = tracker(base);

        let error = tracker
            .record_response(LABEL_A, base)
            .expect_err("no in-flight attempt must fail");

        assert_eq!(
            error,
            HeartbeatError::NoAttemptInFlight(LABEL_A.to_string())
        );
    }

    #[test]
    fn node_state_on_unknown_label_errors() {
        let base = Instant::now();
        let tracker = tracker(base);

        let error = tracker
            .node_state("missing")
            .expect_err("unknown label must fail");

        assert_eq!(error, HeartbeatError::UnknownNode("missing".to_string()));
    }

    #[test]
    fn unavailable_and_states_report_in_order_without_mutating() {
        let base = Instant::now();
        let mut tracker = tracker(base);

        tracker.poll(base);
        tracker.poll(base + timeout());
        tracker.poll(base + timeout() + interval());
        tracker.poll(base + timeout() + interval() + timeout());

        let before = tracker.states().into_iter().cloned().collect::<Vec<_>>();

        assert_eq!(tracker.unavailable(), vec![LABEL_A, LABEL_B]);
        let states = tracker.states();
        assert_eq!(
            states
                .iter()
                .map(|state| state.label.as_str())
                .collect::<Vec<_>>(),
            vec![LABEL_A, LABEL_B]
        );
        assert!(states
            .iter()
            .all(|state| state.availability == Availability::Unavailable));

        let after = tracker.states().into_iter().cloned().collect::<Vec<_>>();
        assert_eq!(before, after);
    }
}
