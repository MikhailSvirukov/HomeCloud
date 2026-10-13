//! Integration tests for heartbeat tracking through the public API, driven
//! entirely by a single `Instant::now()` base plus `Duration` offsets.

use std::num::{NonZeroU32, NonZeroU64};
use std::time::{Duration, Instant};

use dedicated_server::config::{ClusterConfig, ClusterNode, HeartbeatConfig};
use dedicated_server::heartbeat::{
    Availability, HeartbeatAction, HeartbeatError, HeartbeatTracker,
};

const ALPHA: &str = "storage-alpha";
const BRAVO: &str = "storage-bravo";
const UNKNOWN: &str = "storage-unknown";

const INTERVAL_MS: u64 = 1000;
const TIMEOUT_MS: u64 = 300;
const MISS_THRESHOLD: u32 = 2;

fn cluster_of(labels: &[&str]) -> ClusterConfig {
    let nodes = labels
        .iter()
        .enumerate()
        .map(|(position, label)| ClusterNode {
            label: (*label).to_owned(),
            address: format!("10.0.0.{}:7100", position + 1)
                .parse()
                .expect("test address must parse"),
            encryption_public_key: format!("{label}-encryption"),
            signing_public_key: format!("{label}-signing"),
        })
        .collect();
    ClusterConfig { nodes }
}

fn tracker(labels: &[&str], start: Instant) -> HeartbeatTracker {
    let heartbeat = HeartbeatConfig {
        interval_ms: NonZeroU64::new(INTERVAL_MS).expect("INTERVAL_MS is non-zero"),
        timeout_ms: NonZeroU64::new(TIMEOUT_MS).expect("TIMEOUT_MS is non-zero"),
        miss_threshold: NonZeroU32::new(MISS_THRESHOLD).expect("MISS_THRESHOLD is non-zero"),
    };
    HeartbeatTracker::new(&cluster_of(labels), &heartbeat, start)
}

fn interval() -> Duration {
    Duration::from_millis(INTERVAL_MS)
}

fn timeout() -> Duration {
    Duration::from_millis(TIMEOUT_MS)
}

fn send(label: &str) -> HeartbeatAction {
    HeartbeatAction::Send {
        label: label.to_owned(),
    }
}

fn timed_out(label: &str) -> HeartbeatAction {
    HeartbeatAction::TimedOut {
        label: label.to_owned(),
    }
}

fn state_summary(tracker: &HeartbeatTracker) -> Vec<(&str, Availability)> {
    tracker
        .states()
        .iter()
        .map(|state| (state.label.as_str(), state.availability))
        .collect()
}

#[test]
fn poll_sends_then_stays_quiet_in_flight_then_times_out_without_resending() {
    let base = Instant::now();
    let mut tracker = tracker(&[ALPHA, BRAVO], base);

    assert_eq!(tracker.poll(base), vec![send(ALPHA), send(BRAVO)]);
    let in_flight = tracker.poll(base + Duration::from_millis(100));
    assert_eq!(in_flight, Vec::<HeartbeatAction>::new());
    let expired = tracker.poll(base + timeout());
    assert_eq!(expired, vec![timed_out(ALPHA), timed_out(BRAVO)]);
}

#[test]
fn answered_node_stays_healthy_while_silent_node_goes_unavailable_and_recovers() {
    let base = Instant::now();
    let mut tracker = tracker(&[ALPHA, BRAVO], base);
    assert_eq!(tracker.poll(base), vec![send(ALPHA), send(BRAVO)]);

    let first_response = base + Duration::from_millis(100);
    tracker
        .record_response(ALPHA, first_response)
        .expect("alpha has an in-flight attempt");
    assert_eq!(tracker.poll(base + timeout()), vec![timed_out(BRAVO)]);
    assert_eq!(tracker.unavailable(), Vec::<&str>::new());

    let alpha_due = first_response + interval();
    assert_eq!(tracker.poll(alpha_due), vec![send(ALPHA)]);
    tracker
        .record_response(ALPHA, alpha_due + Duration::from_millis(100))
        .expect("alpha has an in-flight attempt");

    let bravo_due = base + timeout() + interval();
    assert_eq!(tracker.poll(bravo_due), vec![send(BRAVO)]);
    let bravo_expired = bravo_due + timeout();
    assert_eq!(tracker.poll(bravo_expired), vec![timed_out(BRAVO)]);

    assert_eq!(tracker.unavailable(), vec![BRAVO]);
    let expected_down = [
        (ALPHA, Availability::Healthy),
        (BRAVO, Availability::Unavailable),
    ];
    assert_eq!(state_summary(&tracker), expected_down);

    let alpha_due_again = alpha_due + Duration::from_millis(100) + interval();
    assert_eq!(tracker.poll(alpha_due_again), vec![send(ALPHA)]);
    tracker
        .record_response(ALPHA, alpha_due_again + Duration::from_millis(100))
        .expect("alpha has an in-flight attempt");

    let bravo_due_again = bravo_expired + interval();
    assert_eq!(tracker.poll(bravo_due_again), vec![send(BRAVO)]);
    tracker
        .record_response(BRAVO, bravo_due_again + Duration::from_millis(100))
        .expect("bravo has an in-flight attempt");

    assert_eq!(tracker.unavailable(), Vec::<&str>::new());
    let expected_recovered = [
        (ALPHA, Availability::Healthy),
        (BRAVO, Availability::Healthy),
    ];
    assert_eq!(state_summary(&tracker), expected_recovered);
}

#[test]
fn record_response_rejects_label_outside_the_cluster() {
    let base = Instant::now();
    let mut tracker = tracker(&[ALPHA, BRAVO], base);

    let error = tracker
        .record_response(UNKNOWN, base)
        .expect_err("unknown label must fail");

    assert_eq!(error, HeartbeatError::UnknownNode(UNKNOWN.to_owned()));
}

#[test]
fn record_response_without_in_flight_attempt_rejects() {
    let base = Instant::now();
    let mut tracker = tracker(&[ALPHA, BRAVO], base);

    let error = tracker
        .record_response(ALPHA, base)
        .expect_err("no attempt is in flight");

    assert_eq!(error, HeartbeatError::NoAttemptInFlight(ALPHA.to_owned()));
}
