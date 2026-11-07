//! Serde support for an update-fragment member that holds a timestamp the
//! fragment may delete (Table 5.2.12-1 `expiresAt`).
//!
//! The timestamp is the clause 4.6.3 `DateTime`, read and written as
//! [`instant_serde`] does; what this adds is the NGSI-LD Null, which deletes
//! the stored timestamp (clause 5.5.8).

use crate::clearable::Clearable;
use andromeda_ngsi_ld_lexicon::time::{date_time::NgsiLdDateTime, instant_serde};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serializer};

/// Deserialize a member that is present: the NGSI-LD Null deletes it, a
/// clause 4.6.3 `DateTime` sets it.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not a string, a
/// JSON `null` included, and a custom error when the string is not a
/// clause 4.6.3 `DateTime`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Clearable<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    Clearable::<NgsiLdDateTime>::deserialize(deserializer).map(|member| member.map(NgsiLdDateTime::get))
}

/// Serialize a member as the fragment wrote it: the timestamp, or the NGSI-LD
/// Null for a deleted member.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S>(value: &Clearable<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value.serialize_with(serializer, instant_serde::required::serialize)
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

    /// Clause 5.5.8: an absent member is left alone, the NGSI-LD Null deletes
    /// it and a timestamp sets it, and each is written back as it was read.
    #[test]
    fn a_timestamp_is_told_absent_from_deleted_from_set() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.at.is_absent());
        let deleted: Carrier = serde_json::from_value(json!({"at": "urn:ngsi-ld:null"})).expect("the Null");
        assert!(deleted.at.is_ngsi_ld_null());
        let set: Carrier = serde_json::from_value(json!({"at": "2030-01-02T03:04:05Z"})).expect("a timestamp");
        pretty_assertions::assert_eq!(
            set.at,
            Clearable::Set(Utc.with_ymd_and_hms(2030, 1, 2, 3, 4, 5).single().expect("a valid instant"))
        );
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&deleted).expect("writes"), json!({"at": "urn:ngsi-ld:null"}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"at": "2030-01-02T03:04:05Z"}));
    }

    #[test]
    fn a_member_that_is_not_a_timestamp_is_refused() {
        for fragment in [json!({"at": "tomorrow"}), json!({"at": 5}), json!({"at": null})] {
            assert!(serde_json::from_value::<Carrier>(fragment).is_err());
        }
    }
}
