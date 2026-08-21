//! A strictly positive duration expressed in whole milliseconds.

use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use thiserror::Error;

/// A duration in milliseconds that clause 5.2.34 requires to be greater than zero.
///
/// `RegistrationManagementInfo` carries two of these, `timeout` and `cooldown`,
/// both typed as "Number, greater than 0". The wire value is read as a signed
/// integer so that a negative number is rejected with the same restriction
/// message as zero, rather than as an opaque unsigned-decoding failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(try_from = "i64", into = "i64")]
pub struct PositiveMilliseconds(NonZeroU32);

/// Why a number cannot be a [`PositiveMilliseconds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PositiveMillisecondsError {
    /// The value was zero or negative.
    #[error("millisecond value must be greater than 0, got {0}")]
    NotPositive(i64),
    /// The value exceeds the largest representable millisecond count.
    #[error("millisecond value {0} exceeds the supported maximum of {max}", max = u32::MAX)]
    TooLarge(i64),
}

impl PositiveMilliseconds {
    /// The number of milliseconds.
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The value as a standard-library duration.
    pub const fn as_duration(self) -> Duration {
        Duration::from_millis(self.0.get() as u64)
    }
}

impl TryFrom<i64> for PositiveMilliseconds {
    type Error = PositiveMillisecondsError;

    fn try_from(value: i64) -> Result<PositiveMilliseconds, PositiveMillisecondsError> {
        if value <= 0 {
            return Err(PositiveMillisecondsError::NotPositive(value));
        }
        let narrowed = u32::try_from(value).map_err(|_| PositiveMillisecondsError::TooLarge(value))?;
        NonZeroU32::new(narrowed)
            .map(PositiveMilliseconds)
            .ok_or(PositiveMillisecondsError::NotPositive(value))
    }
}

// The wire form clause 5.2.34 defines is a JSON number, and `#[serde(into)]`
// needs an owned conversion to it; `derive_more::Into` cannot widen through the
// inner `NonZeroU32`.
impl From<PositiveMilliseconds> for i64 {
    fn from(value: PositiveMilliseconds) -> i64 {
        i64::from(value.0.get())
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
    }

    #[test]
    fn a_value_beyond_u32_is_rejected_as_too_large() {
        let too_large = i64::from(u32::MAX) + 1;
        pretty_assertions::assert_eq!(PositiveMilliseconds::try_from(too_large), Err(PositiveMillisecondsError::TooLarge(too_large)));
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
