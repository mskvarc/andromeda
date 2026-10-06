//! Serde support for an update-fragment member that is held as a value parsed
//! from a JSON string and that the fragment may clear (Table 5.2.12-1
//! `ngsildConformance`).
//!
//! [`non_null_member::deserialize_parsed`](crate::non_null_member::deserialize_parsed)
//! refuses a `null`; this reads it as the request to remove the member
//! (clause 5.5.8), which is what a [`Clearable`] member says.

use crate::clearable::Clearable;
use serde::{Deserialize, Deserializer, Serializer, de::Error as DeserializeError};
use std::{fmt::Display, str::FromStr};

/// Deserialize a member that is present: `null` clears it, a string is parsed
/// into the value it spells.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither `null` nor a
/// string, and a custom error carrying `T`'s parse failure otherwise.
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Clearable<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    Option::<String>::deserialize(deserializer)?
        .map(|text| text.parse().map_err(DeserializeError::custom))
        .transpose()
        .map(|parsed| parsed.map_or(Clearable::Cleared, Clearable::Set))
}

/// Serialize a member as the fragment wrote it: the string `T`'s `Display`
/// writes, or `null` for a cleared member.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S, T>(value: &Clearable<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Display,
{
    match value {
        Clearable::Set(value) => serializer.collect_str(value),
        Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => serializer.serialize_none(),
    }
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
    fn a_parsed_member_is_told_absent_from_cleared_from_set() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.count.is_absent());
        let cleared: Carrier = serde_json::from_value(json!({"count": null})).expect("null");
        assert!(matches!(cleared.count, Clearable::Cleared));
        let set: Carrier = serde_json::from_value(json!({"count": "7"})).expect("a string");
        pretty_assertions::assert_eq!(set.count, Clearable::Set(7));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(serde_json::to_value(&cleared).expect("writes"), json!({"count": null}));
        pretty_assertions::assert_eq!(serde_json::to_value(&set).expect("writes"), json!({"count": "7"}));
    }

    #[test]
    fn a_member_that_is_not_a_parsable_string_is_refused() {
        assert!(serde_json::from_value::<Carrier>(json!({"count": 7})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"count": "seven"})).is_err());
    }
}
