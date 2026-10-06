//! Request-body members that may be left out but may not be written as `null`.
//!
//! A reader that maps `null` to the absent member accepts `"entities": null` as
//! "no entities", which clause 5.5.4 does not allow for a member whose data type
//! is an array: the value is present and it is not that data type. The carrier
//! field is an `Option<T>` with `#[serde(default)]`, so that an absent member is
//! `None` and a `null` is the reader's own refusal.

use serde::{Deserialize, Deserializer, Serializer, de::Error as DeserializeError};
use std::{fmt::Display, str::FromStr};

/// Deserialize a member that is present as the `T` its data type names.
///
/// # Errors
///
/// Returns `T`'s own refusal, which a `null` draws like any value outside the
/// data type.
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Deserialize a member that is present as the JSON string a `T` is parsed from.
///
/// # Errors
///
/// Returns the reader's refusal when the member is not a string, `null`
/// included, and a custom error carrying `T`'s parse failure otherwise.
pub fn deserialize_parsed<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    String::deserialize(deserializer)?.parse().map(Some).map_err(DeserializeError::custom)
}

/// Serialize a member as the string its `Display` writes, and an absent one as
/// `null`.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize_displayed<S, T>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Display,
{
    match value {
        Some(value) => serializer.collect_str(value),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use crate::non_null_member;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for any resource that owns such members.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_null_member::deserialize")]
        names: Option<Vec<String>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "non_null_member::deserialize_parsed",
            serialize_with = "non_null_member::serialize_displayed"
        )]
        count: Option<u32>,
    }

    #[test]
    fn an_absent_member_is_none_and_a_present_one_is_read() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent members are fine");
        assert!(absent.names.is_none() && absent.count.is_none());
        let present: Carrier = serde_json::from_value(json!({"names": ["a"], "count": "7"})).expect("present members are read");
        pretty_assertions::assert_eq!(present.names, Some(vec!["a".to_owned()]));
        pretty_assertions::assert_eq!(present.count, Some(7));
        pretty_assertions::assert_eq!(serde_json::to_value(&present).expect("serialises"), json!({"names": ["a"], "count": "7"}));
    }

    #[test]
    fn a_null_member_is_refused_rather_than_read_as_absent() {
        assert!(serde_json::from_value::<Carrier>(json!({"names": null})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"count": null})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"count": 7})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"count": "seven"})).is_err());
    }
}
