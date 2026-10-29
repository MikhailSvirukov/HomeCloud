//! Inbound frame sequence validation.
//!
//! SRS: `INT-NET-008 / 2`.

/// Tracks the next expected frame number for one sequence.
#[derive(Debug)]
pub struct FrameSequence {
    next: Option<u32>,
}

impl Default for FrameSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameSequence {
    /// Creates a sequence that expects frame number `0` first.
    pub const fn new() -> Self {
        Self { next: Some(0) }
    }

    /// Accepts the next frame number; the first accepted value is `0`.
    pub fn accept(&mut self, frame_number: u32) -> Result<(), FrameSequenceError> {
        let Some(expected) = self.next else {
            return Err(FrameSequenceError::Exhausted);
        };
        if frame_number != expected {
            return Err(FrameSequenceError::OutOfOrder {
                expected,
                actual: frame_number,
            });
        }

        self.next = frame_number.checked_add(1);
        Ok(())
    }
}

/// Rejection reasons for an inbound frame number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameSequenceError {
    /// The frame number does not equal the next expected value.
    #[error("expected frame number {expected}, got {actual}")]
    OutOfOrder { expected: u32, actual: u32 },
    /// The sequence already accepted `u32::MAX`.
    #[error("the frame-number space is exhausted")]
    Exhausted,
}

#[cfg(test)]
mod tests {
    use super::{FrameSequence, FrameSequenceError};

    #[test]
    fn accepting_the_maximum_frame_number_exhausts_the_sequence() {
        let mut sequence = FrameSequence {
            next: Some(u32::MAX),
        };

        assert_eq!(sequence.accept(u32::MAX), Ok(()));
        assert_eq!(sequence.accept(0), Err(FrameSequenceError::Exhausted));
    }
}
