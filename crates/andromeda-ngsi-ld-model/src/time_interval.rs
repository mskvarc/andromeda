//! The TimeInterval member (ETSI GS CIM 009 clause 5.2.11).

use andromeda_ngsi_ld_lexicon::rfc3339;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A registration's `observationInterval` or `managementInterval`: the span of
/// time the registered source can answer temporal queries for (Table 5.2.11-1).
///
/// `startAt` is mandatory; an absent `endAt` leaves the interval open. A closed
/// interval that ends before it starts describes no time at all, so it is
/// rejected at deserialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "TimeIntervalWire")]
pub struct TimeInterval {
    /// The start of the interval.
    #[serde(with = "rfc3339::datetime")]
    pub start_at: DateTime<Utc>,
    /// The end of the interval; open when absent.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub end_at: Option<DateTime<Utc>>,
}

/// Why a wire document cannot be a [`TimeInterval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimeIntervalError {
    /// `endAt` was not after `startAt`.
    #[error("endAt must be after startAt")]
    EndNotAfterStart,
}

/// The clause 5.2.11 wire shape before the ordering restriction is checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeIntervalWire {
    /// The `startAt` member.
    #[serde(with = "rfc3339::datetime")]
    start_at: DateTime<Utc>,
    /// The `endAt` member.
    #[serde(with = "rfc3339::optional_datetime", default)]
    end_at: Option<DateTime<Utc>>,
}

impl TryFrom<TimeIntervalWire> for TimeInterval {
    type Error = TimeIntervalError;

    fn try_from(wire: TimeIntervalWire) -> Result<TimeInterval, TimeIntervalError> {
        if wire.end_at.is_some_and(|end_at| end_at <= wire.start_at) {
            return Err(TimeIntervalError::EndNotAfterStart);
        }
        Ok(TimeInterval {
            start_at: wire.start_at,
            end_at: wire.end_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::time_interval::TimeInterval;
    use serde_json::json;

    #[test]
    fn a_closed_interval_round_trips() {
        let wire = json!({"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"});
        let parsed: TimeInterval = serde_json::from_value(wire.clone()).expect("interval parses"); // clone: compared below
        assert!(parsed.end_at.is_some());
        pretty_assertions::assert_eq!(serde_json::to_value(parsed).expect("serialises"), wire);
    }

    #[test]
    fn an_interval_with_only_start_at_is_open_ended() {
        let parsed: TimeInterval = serde_json::from_value(json!({"startAt": "2020-01-01T00:00:00Z"})).expect("interval parses");
        pretty_assertions::assert_eq!(parsed.end_at, None);
        pretty_assertions::assert_eq!(serde_json::to_value(parsed).expect("serialises"), json!({"startAt": "2020-01-01T00:00:00Z"}));
    }

    #[test]
    fn a_missing_start_at_is_rejected() {
        assert!(serde_json::from_value::<TimeInterval>(json!({"endAt": "2030-01-01T00:00:00Z"})).is_err());
    }

    #[test]
    fn an_end_at_not_after_start_at_is_rejected() {
        assert!(serde_json::from_value::<TimeInterval>(json!({"startAt": "2021-01-01T00:00:00Z", "endAt": "2020-01-01T00:00:00Z"})).is_err());
        assert!(serde_json::from_value::<TimeInterval>(json!({"startAt": "2021-01-01T00:00:00Z", "endAt": "2021-01-01T00:00:00Z"})).is_err());
    }
}
