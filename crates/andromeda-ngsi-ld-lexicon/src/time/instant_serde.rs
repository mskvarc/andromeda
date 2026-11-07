//! Reading and writing a `DateTime<Utc>` member through `#[serde(with = …)]`.
//!
//! A member typed as an instant is read by [`NgsiLdDateTime`]'s rules and
//! written as [`crate::time::rendering::instant_text`] spells it, so a serde model holding a bare
//! `DateTime<Utc>` follows the same clause 4.6.3 rules as every other path.
//! `chrono`'s own serde support is not enabled anywhere in the workspace, so a
//! member that names neither adapter does not compile.

use crate::time::date_time::NgsiLdDateTime;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

/// Read one instant from a deserializer.
fn read<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    NgsiLdDateTime::deserialize(deserializer).map(NgsiLdDateTime::get)
}

/// A required instant.
pub mod required {
    use crate::time::{instant_serde::read, rendering::instant_text};
    use chrono::{DateTime, Utc};
    use serde::{Deserializer, Serializer};

    /// Write the instant as [`instant_text`] spells it.
    ///
    /// # Errors
    ///
    /// Propagates whatever the serializer reports.
    pub fn serialize<S>(instant: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&instant_text(*instant))
    }

    /// Read a clause 4.6.3 `DateTime`.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when the value is not a string or is not a
    /// clause 4.6.3 `DateTime`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        read(deserializer)
    }
}

/// An optional instant: absent or `null` is `None`.
pub mod optional {
    use crate::time::{date_time::NgsiLdDateTime, instant_serde::required};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Write the instant when present, and `null` when absent.
    ///
    /// # Errors
    ///
    /// Propagates whatever the serializer reports.
    pub fn serialize<S>(instant: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match instant {
            Some(instant) => required::serialize(instant, serializer),
            None => serializer.serialize_none(),
        }
    }

    /// Read an optional clause 4.6.3 `DateTime`.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when a value is present and is not a clause
    /// 4.6.3 `DateTime`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<NgsiLdDateTime>::deserialize(deserializer)?.map(NgsiLdDateTime::get))
    }
}

#[cfg(test)]
mod tests {
    use crate::time::rendering::instant_text;
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Required {
        #[serde(with = "crate::time::instant_serde::required")]
        at: DateTime<Utc>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Optional {
        #[serde(with = "crate::time::instant_serde::optional", skip_serializing_if = "Option::is_none", default)]
        at: Option<DateTime<Utc>>,
    }

    fn instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).single().expect("a valid instant") + TimeDelta::milliseconds(750)
    }

    #[test]
    fn both_adapters_write_the_one_instant_spelling() {
        let expected = serde_json::Value::String(instant_text(instant()));
        pretty_assertions::assert_eq!(serde_json::to_value(Required { at: instant() }).expect("serializable")["at"], expected);
        pretty_assertions::assert_eq!(serde_json::to_value(Optional { at: Some(instant()) }).expect("serializable")["at"], expected);
    }

    #[test]
    fn both_adapters_round_trip() {
        let required: Required = serde_json::from_str(&serde_json::to_string(&Required { at: instant() }).expect("serializable")).expect("readable");
        pretty_assertions::assert_eq!(required.at, instant());
        let optional: Optional = serde_json::from_str(&serde_json::to_string(&Optional { at: Some(instant()) }).expect("serializable")).expect("readable");
        pretty_assertions::assert_eq!(optional.at, Some(instant()));
        let absent: Optional = serde_json::from_str("{}").expect("an absent member");
        pretty_assertions::assert_eq!(absent.at, None);
        let null: Optional = serde_json::from_str(r#"{"at": null}"#).expect("a null member");
        pretty_assertions::assert_eq!(null.at, None);
    }

    #[test]
    fn a_comma_separator_is_read() {
        let read: Required = serde_json::from_str(r#"{"at": "2024-06-15T12:30:00,75Z"}"#).expect("the comma form");
        pretty_assertions::assert_eq!(read.at, instant());
    }

    /// Clause 4.6.3 holds on every member these adapters read.
    #[test]
    fn both_adapters_refuse_what_clause_4_6_3_refuses() {
        for text in [
            "2024-06-15T12:30:00+01:00",
            "2024-06-15T12:30:00+00:00",
            "2024-06-15T12:30:00.1234567Z",
            "2024-06-15T12:30Z",
            "2024-06-15T12:30:00z",
        ] {
            let json = format!(r#"{{"at": "{text}"}}"#);
            assert!(serde_json::from_str::<Required>(&json).is_err(), "{text}");
            assert!(serde_json::from_str::<Optional>(&json).is_err(), "{text}");
        }
    }
}
