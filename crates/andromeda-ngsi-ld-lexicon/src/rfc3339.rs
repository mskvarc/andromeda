//! The RFC 3339 rendering of the timestamp members Andromeda writes itself.
//!
//! Chrono's own `Serialize` emits sub-second precision that varies with the
//! stored value. Andromeda's stable outbound policy is whole-second UTC
//! rendering for every timestamp it generates; this is a broker policy, not an
//! ETSI requirement about stored or accepted timestamp precision. The serde
//! adapters below are that same rendering reached through `#[serde(with = …)]`;
//! a caller building a timestamp into a JSON value by hand calls the function
//! directly.

use chrono::{DateTime, SecondsFormat, Utc};

/// Render a UTC timestamp as RFC 3339 with whole-second precision.
///
/// The single reading of the format every NGSI-LD timestamp Andromeda
/// generates is written in: `2024-06-15T12:30:00Z`, never a sub-second
/// fraction and never a numeric offset.
#[must_use]
pub fn whole_seconds(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Serialize and deserialize a required `DateTime<Utc>` as an RFC 3339 string.
pub mod datetime {
    use crate::rfc3339::whole_seconds;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    /// Render the timestamp as an RFC 3339 string with whole-second precision.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&whole_seconds(*dt))
    }

    /// Parse an RFC 3339 string into a UTC timestamp.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when the value is not a string or does not
    /// parse as RFC 3339.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).map_err(Error::custom)
    }
}

/// Serialize and deserialize an optional `DateTime<Utc>` as an RFC 3339 string.
pub mod optional_datetime {
    use crate::rfc3339::whole_seconds;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    /// Render the timestamp when present, and JSON `null` when absent.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(opt: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(dt) => serializer.serialize_some(&whole_seconds(*dt)),
            None => serializer.serialize_none(),
        }
    }

    /// Parse an optional RFC 3339 string into a UTC timestamp.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when a value is present but does not parse
    /// as RFC 3339.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => DateTime::parse_from_rfc3339(&s).map(|dt| Some(dt.with_timezone(&Utc))).map_err(Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rfc3339::whole_seconds;
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestRequired {
        #[serde(with = "super::datetime")]
        ts: DateTime<Utc>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestOptional {
        #[serde(with = "super::optional_datetime", skip_serializing_if = "Option::is_none", default)]
        ts: Option<DateTime<Utc>>,
    }

    #[test]
    fn a_sub_second_timestamp_renders_at_whole_seconds_with_a_z_suffix() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap() + chrono::Duration::microseconds(123_456);
        pretty_assertions::assert_eq!(whole_seconds(dt), "2024-06-15T12:30:00Z");
    }

    #[test]
    fn the_serde_adapters_render_exactly_what_the_function_does() {
        // Two readings of one format is how they drift; this pins them to one.
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap() + chrono::Duration::milliseconds(750);
        let expected = serde_json::Value::String(whole_seconds(dt));

        let required = serde_json::to_value(TestRequired { ts: dt }).unwrap();
        pretty_assertions::assert_eq!(required["ts"], expected);

        let optional = serde_json::to_value(TestOptional { ts: Some(dt) }).unwrap();
        pretty_assertions::assert_eq!(optional["ts"], expected);
    }

    #[test]
    fn rfc3339_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestRequired { ts: dt };
        let json = serde_json::to_string(&val).unwrap();
        assert!(json.contains("2024-06-15"));
        let parsed: TestRequired = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, dt);
    }

    #[test]
    fn rfc3339_option_some_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestOptional { ts: Some(dt) };
        let json = serde_json::to_string(&val).unwrap();
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, Some(dt));
    }

    #[test]
    fn rfc3339_option_none_round_trip() {
        let val = TestOptional { ts: None };
        let json = serde_json::to_string(&val).unwrap();
        assert!(!json.contains("ts"));
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, None);
    }
}
