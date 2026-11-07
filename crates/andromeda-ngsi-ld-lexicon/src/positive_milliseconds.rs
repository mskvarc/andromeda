//! A strictly positive duration expressed in whole milliseconds.

use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroI64, time::Duration};
use thiserror::Error;

/// A duration in milliseconds the spec restricts to "Greater than 0".
///
/// Two data types carry a pair of these under the same restriction: the
/// `RegistrationManagementInfo` of Table 5.2.34-1 and the `Endpoint` of
/// Table 5.2.15-1, each with a `cooldown` and a `timeout`. The specification
/// sets no upper bound, so every positive signed 64-bit count is admitted: the
/// same range a PostgreSQL `bigint` and a `chrono` time delta hold, about 292
/// million years. The wire value is read as a signed integer so that a
/// negative number is rejected with the same restriction message as zero,
/// rather than as an opaque unsigned-decoding failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "i64", into = "i64")]
pub struct PositiveMilliseconds(NonZeroI64);

/// Why a number cannot be a [`PositiveMilliseconds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PositiveMillisecondsError {
    /// The value was zero or negative.
    #[error("millisecond value must be greater than 0, got {0}")]
    NotPositive(i64),
}

impl PositiveMilliseconds {
    /// The number of milliseconds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get().unsigned_abs()
    }

    /// The value as a standard-library duration.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        Duration::from_millis(self.get())
    }
}

impl TryFrom<i64> for PositiveMilliseconds {
    type Error = PositiveMillisecondsError;

    fn try_from(value: i64) -> Result<PositiveMilliseconds, PositiveMillisecondsError> {
        NonZeroI64::new(value)
            .filter(|count| count.is_positive())
            .map(PositiveMilliseconds)
            .ok_or(PositiveMillisecondsError::NotPositive(value))
    }
}

// The wire form clause 5.2.34 defines is a JSON number, and `#[serde(into)]`
// needs an owned conversion to it; `derive_more::Into` cannot unwrap through
// the inner `NonZeroI64`.
impl From<PositiveMilliseconds> for i64 {
    fn from(value: PositiveMilliseconds) -> i64 {
        value.0.get()
    }
}

#[cfg(test)]
mod tests {
    use crate::positive_milliseconds::{PositiveMilliseconds, PositiveMillisecondsError};
    use std::time::Duration;

    #[test]
    fn a_positive_value_is_accepted_and_converts_to_a_duration() {
        let value = PositiveMilliseconds::try_from(1500).expect("positive value");
        pretty_assertions::assert_eq!(value.get(), 1500);
        pretty_assertions::assert_eq!(value.as_duration(), Duration::from_millis(1500));
        pretty_assertions::assert_eq!(value.to_string(), "1500");
    }

    #[test]
    fn zero_and_negative_values_are_rejected_as_not_positive() {
        pretty_assertions::assert_eq!(PositiveMilliseconds::try_from(0), Err(PositiveMillisecondsError::NotPositive(0)));
        pretty_assertions::assert_eq!(PositiveMilliseconds::try_from(-5), Err(PositiveMillisecondsError::NotPositive(-5)));
        pretty_assertions::assert_eq!(PositiveMilliseconds::try_from(i64::MIN), Err(PositiveMillisecondsError::NotPositive(i64::MIN)));
    }

    #[test]
    fn a_value_beyond_u32_round_trips_through_json() {
        let parsed: PositiveMilliseconds = serde_json::from_str("4294967296000").expect("a count above u32 parses");
        pretty_assertions::assert_eq!(parsed.get(), 4_294_967_296_000);
        pretty_assertions::assert_eq!(parsed.as_duration(), Duration::from_secs(4_294_967_296));
        pretty_assertions::assert_eq!(serde_json::to_string(&parsed).expect("serialises"), "4294967296000");
    }

    #[test]
    fn the_largest_signed_count_is_accepted_and_one_past_it_is_refused() {
        let widest: PositiveMilliseconds = serde_json::from_str("9223372036854775807").expect("i64::MAX parses");
        pretty_assertions::assert_eq!(i64::from(widest), i64::MAX);
        assert!(serde_json::from_str::<PositiveMilliseconds>("9223372036854775808").is_err());
    }

    #[test]
    fn serde_reads_a_json_number_and_rejects_zero_negative_and_non_integers() {
        let parsed: PositiveMilliseconds = serde_json::from_str("3000").expect("positive number parses");
        pretty_assertions::assert_eq!(parsed.get(), 3000);
        pretty_assertions::assert_eq!(serde_json::to_string(&parsed).expect("serialises"), "3000");
        assert!(serde_json::from_str::<PositiveMilliseconds>("0").is_err());
        assert!(serde_json::from_str::<PositiveMilliseconds>("-1").is_err());
        assert!(serde_json::from_str::<PositiveMilliseconds>("1.5").is_err());
        assert!(serde_json::from_str::<PositiveMilliseconds>("\"10\"").is_err());
    }
}
