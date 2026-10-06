//! Serde support for the `timeInterval` and `throttling` members, which
//! Table 5.2.12-1 types as a number of seconds.
//!
//! The members are held as floating-point numbers because a client may ask for
//! a fraction of a second, so a subscription written with `5` would be written
//! back as `5.0`. A whole number of seconds is written as the integer it was
//! given as, and a fraction as the fraction.

use serde::Serializer;

/// The whole-second value of `seconds`, or `None` when it carries a fraction
/// or falls outside the range an integer JSON number can hold.
///
/// The conversion goes through the decimal rendering rather than a cast so
/// that a value too large for [`i64`] keeps its floating-point form instead of
/// being truncated into range.
fn whole_seconds(seconds: f64) -> Option<i64> {
    if seconds.fract() != 0.0 {
        return None;
    }
    format!("{seconds:.0}").parse().ok()
}

/// Serialize an optional number of seconds, a whole one as an integer.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S>(seconds: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match *seconds {
        Some(seconds) => match whole_seconds(seconds) {
            Some(whole) => serializer.serialize_i64(whole),
            None => serializer.serialize_f64(seconds),
        },
        None => serializer.serialize_none(),
    }
}

/// The form of a seconds member an update fragment may clear.
///
/// Clause 5.5.8: an explicit `null` removes the stored member, which is what a
/// [`Clearable`](crate::clearable::Clearable) member says. Reading is the
/// number [`Clearable`](crate::clearable::Clearable) reads by itself; writing
/// is [`serialize`](super::serialize).
pub mod clearable {
    use crate::{clearable::Clearable, delivery_seconds};
    use serde::Serializer;

    /// Serialize a member as the fragment wrote it: the number of seconds, a
    /// whole one as an integer, or `null` for a cleared member.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when the member cannot be written.
    pub fn serialize<S>(seconds: &Clearable<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        delivery_seconds::serialize(&seconds.as_set().copied(), serializer)
    }
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, delivery_seconds};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for a resource that owns such a member.
    #[derive(Serialize)]
    struct Carrier {
        #[serde(skip_serializing_if = "Option::is_none", serialize_with = "delivery_seconds::serialize")]
        seconds: Option<f64>,
    }

    fn written(seconds: Option<f64>) -> serde_json::Value {
        serde_json::to_value(Carrier { seconds }).expect("the member serialises")
    }

    #[test]
    fn a_whole_number_of_seconds_is_written_as_an_integer() {
        pretty_assertions::assert_eq!(written(Some(5.0)), json!({"seconds": 5}));
    }

    #[test]
    fn a_fraction_of_a_second_stays_a_fraction() {
        pretty_assertions::assert_eq!(written(Some(0.5)), json!({"seconds": 0.5}));
    }

    #[test]
    fn a_value_past_the_integer_range_keeps_its_floating_point_form() {
        pretty_assertions::assert_eq!(written(Some(1.0e30)), json!({"seconds": 1.0e30}));
    }

    #[test]
    fn an_absent_member_is_not_written() {
        pretty_assertions::assert_eq!(written(None), json!({}));
    }

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct FragmentCarrier {
        #[serde(default, skip_serializing_if = "Clearable::is_absent", serialize_with = "delivery_seconds::clearable::serialize")]
        seconds: Clearable<f64>,
    }

    /// Clause 5.5.8: an absent member is left alone, a `null` clears it and a
    /// number sets it, a whole one written back as an integer.
    #[test]
    fn a_fragment_member_is_told_absent_from_cleared_from_set() {
        let absent: FragmentCarrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.seconds.is_absent());
        let cleared: FragmentCarrier = serde_json::from_value(json!({"seconds": null})).expect("null");
        assert!(matches!(cleared.seconds, Clearable::Cleared));
        let set: FragmentCarrier = serde_json::from_value(json!({"seconds": 5})).expect("a number");
        pretty_assertions::assert_eq!(set.seconds, Clearable::Set(5.0));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&cleared).expect("writes"), json!({"seconds": null}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"seconds": 5}));
    }
}
