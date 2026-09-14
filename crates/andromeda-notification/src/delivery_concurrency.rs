//! How many claimed notification deliveries one drain pass sends at once.
//!
//! A pass claims a batch and then waits on the transport for each delivery;
//! sending them one after another makes the pass as slow as the sum of every
//! endpoint's latency. The concurrency bound is the number of deliveries kept
//! in flight together, and is capped so a single pass cannot open an unbounded
//! number of outbound connections.

use derive_more::Display;
use std::num::NonZeroUsize;
use thiserror::Error;

/// Rejected notification delivery concurrency.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryConcurrencyError {
    /// Zero in-flight deliveries would make every pass send nothing.
    #[error("notification delivery concurrency must be at least one")]
    Zero,
    /// A bound above the maximum would let one pass open more outbound
    /// connections than a single worker should hold.
    #[error("notification delivery concurrency of {concurrency} exceeds the maximum of {maximum}")]
    TooHigh {
        /// The rejected bound.
        concurrency: u32,
        /// The largest accepted bound.
        maximum: u32,
    },
}

/// The number of claimed deliveries a drain pass keeps in flight together.
#[derive(Clone, Copy, Debug, Display, Eq, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct DeliveryConcurrency(NonZeroUsize);

impl DeliveryConcurrency {
    /// The largest accepted concurrency.
    pub const MAXIMUM: u32 = 1_024;

    /// Build a concurrency bound from a whole number of in-flight deliveries.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryConcurrencyError`] when the number is zero or exceeds
    /// [`DeliveryConcurrency::MAXIMUM`].
    pub const fn from_count(concurrency: u32) -> Result<DeliveryConcurrency, DeliveryConcurrencyError> {
        if concurrency > DeliveryConcurrency::MAXIMUM {
            return Err(DeliveryConcurrencyError::TooHigh {
                concurrency,
                maximum: DeliveryConcurrency::MAXIMUM,
            });
        }
        match NonZeroUsize::new(concurrency as usize) {
            Some(concurrency) => Ok(DeliveryConcurrency(concurrency)),
            None => Err(DeliveryConcurrencyError::Zero),
        }
    }

    /// The bound as the number of deliveries kept in flight.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_concurrency::{DeliveryConcurrency, DeliveryConcurrencyError};
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_concurrency_is_rejected() {
        assert_eq!(DeliveryConcurrency::from_count(0), Err(DeliveryConcurrencyError::Zero));
    }

    #[test]
    fn concurrency_above_the_maximum_is_rejected() {
        assert_eq!(
            DeliveryConcurrency::from_count(1_025),
            Err(DeliveryConcurrencyError::TooHigh {
                concurrency: 1_025,
                maximum: 1_024,
            })
        );
    }

    #[test]
    fn concurrency_reports_its_in_flight_count() -> Result<(), DeliveryConcurrencyError> {
        assert_eq!(DeliveryConcurrency::from_count(20)?.get(), 20);
        Ok(())
    }
}
