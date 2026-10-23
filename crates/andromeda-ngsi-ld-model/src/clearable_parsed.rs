//! Serde support for an update-fragment member that is held as a value parsed
//! from a JSON string (Table 5.2.12-1 `ngsildConformance`).
//!
//! The string is the NGSI-LD Null, which deletes the member (clause 5.5.8), or
//! the value it spells.

use crate::clearable::Clearable;
use serde::{Deserialize, Deserializer, Serializer, de::Error as DeserializeError};
use std::{fmt::Display, str::FromStr};

/// Deserialize a member that is present: the NGSI-LD Null deletes it, any
/// other string is parsed into the value it spells.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not a string, a
/// JSON `null` included, and a custom error carrying `T`'s parse failure
/// otherwise.
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Clearable<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    let text = String::deserialize(deserializer)?;
    Clearable::from_text(&text, str::parse).map_err(DeserializeError::custom)
}

/// Serialize a member as the fragment wrote it: the string `T`'s `Display`
/// writes, or the NGSI-LD Null for a deleted member.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S, T>(value: &Clearable<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Display,
{
    value.serialize_with(serializer, |value, serializer| serializer.collect_str(value))
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, clearable_parsed};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(
            default,
            skip_serializing_if = "Clearable::is_absent",
            deserialize_with = "clearable_parsed::deserialize",
            serialize_with = "clearable_parsed::serialize"
        )]
        count: Clearable<u32>,
    }

    #[test]
    fn a_parsed_member_is_told_absent_from_deleted_from_set() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.count.is_absent());
        let deleted: Carrier = serde_json::from_value(json!({"count": "urn:ngsi-ld:null"})).expect("the Null");
        assert!(deleted.count.is_ngsi_ld_null());
        let set: Carrier = serde_json::from_value(json!({"count": "7"})).expect("a string");
        pretty_assertions::assert_eq!(set.count, Clearable::Set(7));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&deleted).expect("writes"), json!({"count": "urn:ngsi-ld:null"}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"count": "7"}));
    }

    #[test]
    fn a_member_that_is_not_a_parsable_string_is_refused() {
        for fragment in [json!({"count": 7}), json!({"count": "seven"}), json!({"count": null})] {
            assert!(serde_json::from_value::<Carrier>(fragment).is_err());
        }
    }
}
