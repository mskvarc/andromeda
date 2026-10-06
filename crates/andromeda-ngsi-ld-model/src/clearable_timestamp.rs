//! Serde support for an update-fragment member that holds a timestamp the
//! fragment may clear (Table 5.2.12-1 `expiresAt`).
//!
//! The timestamp is the RFC 3339 string clause 4.6.3 defines, written and read
//! as [`rfc3339::optional_datetime`] does; what this adds is the third state of
//! a [`Clearable`] member, so that a `null` removes the stored timestamp
//! (clause 5.5.8) instead of being read as an absent member.

use crate::clearable::Clearable;
use andromeda_ngsi_ld_lexicon::rfc3339;
use chrono::{DateTime, Utc};
use serde::{Deserializer, Serializer};

/// Deserialize a member that is present: `null` clears it, an RFC 3339 string
/// sets it.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither `null` nor
/// a string, and a custom error when the string is not RFC 3339.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Clearable<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(rfc3339::optional_datetime::deserialize(deserializer)?.map_or(Clearable::Cleared, Clearable::Set))
}

/// Serialize a member as the fragment wrote it: the RFC 3339 string, or `null`
/// for a cleared member.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S>(value: &Clearable<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    rfc3339::optional_datetime::serialize(&value.as_set().copied(), serializer)
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, clearable_timestamp};
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(
            default,
            skip_serializing_if = "Clearable::is_absent",
            deserialize_with = "clearable_timestamp::deserialize",
            serialize_with = "clearable_timestamp::serialize"
        )]
        at: Clearable<DateTime<Utc>>,
    }

    /// Clause 5.5.8: an absent member is left alone, a `null` clears it and a
    /// string sets it, and each is written back as it was read.
    #[test]
    fn a_timestamp_is_told_absent_from_cleared_from_set() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.at.is_absent());
        let cleared: Carrier = serde_json::from_value(json!({"at": null})).expect("null");
        assert!(matches!(cleared.at, Clearable::Cleared));
        let set: Carrier = serde_json::from_value(json!({"at": "2030-01-02T03:04:05Z"})).expect("a timestamp");
        pretty_assertions::assert_eq!(
            set.at,
            Clearable::Set(Utc.with_ymd_and_hms(2030, 1, 2, 3, 4, 5).single().expect("a valid instant"))
        );
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&cleared).expect("writes"), json!({"at": null}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"at": "2030-01-02T03:04:05Z"}));
    }

    #[test]
    fn a_member_that_is_not_a_timestamp_is_refused() {
        assert!(serde_json::from_value::<Carrier>(json!({"at": "tomorrow"})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"at": 5})).is_err());
    }
}
