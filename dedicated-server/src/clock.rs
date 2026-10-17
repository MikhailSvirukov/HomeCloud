//! Clock offset estimation for heartbeat exchanges.

/// The four millisecond timestamps of one heartbeat exchange.
///
/// `sent_at_ms` is when this server sent the request, `node_received_at_ms`
/// when the node received it, `node_sent_at_ms` when the node sent the answer
/// and `completed_at_ms` when this server received that answer. All values are
/// Unix milliseconds from the respective host clock.
pub struct ExchangeTimestamps {
    pub sent_at_ms: i64,
    pub node_received_at_ms: i64,
    pub node_sent_at_ms: i64,
    pub completed_at_ms: i64,
}

/// Clock offset and round-trip delay estimated from one exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    /// Node clock minus this server's clock, in milliseconds.
    pub offset_ms: i64,
    /// Round-trip delay excluding the node's own processing time.
    pub rtt_ms: i64,
}

impl ExchangeTimestamps {
    /// Estimate the sample of this exchange, discarding a negative delay.
    pub fn sample(&self) -> Option<ClockSample> {
        let rtt_ms = (self.completed_at_ms - self.sent_at_ms)
            - (self.node_sent_at_ms - self.node_received_at_ms);
        if rtt_ms < 0 {
            return None;
        }

        let offset_ms = ((self.node_received_at_ms - self.sent_at_ms)
            + (self.node_sent_at_ms - self.completed_at_ms))
            / 2;

        Some(ClockSample { offset_ms, rtt_ms })
    }
}

/// Offset estimate to carry into the next heartbeat request.
#[derive(Debug, Default)]
pub struct ClockOffset {
    previous: Option<i64>,
}

impl ClockOffset {
    /// Estimate a sample and remember its offset when the sample is valid.
    pub fn observe(&mut self, timestamps: &ExchangeTimestamps) -> Option<ClockSample> {
        let sample = timestamps.sample()?;
        self.previous = Some(sample.offset_ms);
        Some(sample)
    }

    /// Offset to send with the next request, absent until a valid sample exists.
    pub fn previous(&self) -> Option<i64> {
        self.previous
    }
}

#[cfg(test)]
mod tests {
    use super::{ClockOffset, ClockSample, ExchangeTimestamps};

    #[test]
    fn symmetric_exchange_yields_zero_offset_and_double_one_way_rtt() {
        let timestamps = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_050,
            node_sent_at_ms: 1_050,
            completed_at_ms: 1_100,
        };

        assert_eq!(
            timestamps.sample(),
            Some(ClockSample {
                offset_ms: 0,
                rtt_ms: 100,
            })
        );
    }

    #[test]
    fn node_clock_ahead_yields_positive_offset() {
        let timestamps = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_700,
            node_sent_at_ms: 1_800,
            completed_at_ms: 1_300,
        };

        assert_eq!(
            timestamps.sample(),
            Some(ClockSample {
                offset_ms: 600,
                rtt_ms: 200,
            })
        );
    }

    #[test]
    fn node_clock_behind_yields_negative_offset() {
        let timestamps = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 600,
            node_sent_at_ms: 600,
            completed_at_ms: 1_200,
        };

        assert_eq!(
            timestamps.sample(),
            Some(ClockSample {
                offset_ms: -500,
                rtt_ms: 200,
            })
        );
    }

    #[test]
    fn negative_odd_numerator_truncates_toward_zero() {
        let timestamps = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_000,
            node_sent_at_ms: 1_200,
            completed_at_ms: 1_203,
        };

        assert_eq!(
            timestamps.sample(),
            Some(ClockSample {
                offset_ms: -1,
                rtt_ms: 3,
            })
        );
    }

    #[test]
    fn negative_delay_discards_sample() {
        let timestamps = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_050,
            node_sent_at_ms: 1_300,
            completed_at_ms: 1_200,
        };

        assert_eq!(timestamps.sample(), None);
    }

    #[test]
    fn offset_state_tracks_latest_valid_sample() {
        let mut offset = ClockOffset::default();
        assert_eq!(offset.previous(), None);

        let valid = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_700,
            node_sent_at_ms: 1_800,
            completed_at_ms: 1_300,
        };
        assert_eq!(
            offset.observe(&valid).map(|sample| sample.offset_ms),
            Some(600)
        );
        assert_eq!(offset.previous(), Some(600));

        let invalid = ExchangeTimestamps {
            sent_at_ms: 1_000,
            node_received_at_ms: 1_050,
            node_sent_at_ms: 1_300,
            completed_at_ms: 1_200,
        };
        assert_eq!(offset.observe(&invalid), None);
        assert_eq!(offset.previous(), Some(600));

        let later = ExchangeTimestamps {
            sent_at_ms: 2_000,
            node_received_at_ms: 1_900,
            node_sent_at_ms: 1_900,
            completed_at_ms: 2_200,
        };
        assert_eq!(
            offset.observe(&later).map(|sample| sample.offset_ms),
            Some(-200)
        );
        assert_eq!(offset.previous(), Some(-200));
    }
}
