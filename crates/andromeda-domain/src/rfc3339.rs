//! RFC 3339 serde adapters for the timestamp members Andromeda writes itself.
//!
//! Chrono's own `Serialize` emits sub-second precision that varies with the
//! stored value; NGSI-LD timestamps are compared as strings by clients, so the
//! members that Andromeda generates go through these adapters to fix the
//! rendering at whole seconds with a `Z` suffix.

/// Serialize and deserialize a required `DateTime<Utc>` as an RFC 3339 string.
pub mod datetime {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    /// Render the timestamp as an RFC 3339 string with whole-second precision.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Secs, true))
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
        DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).map_err(D::Error::custom)
    }
}

/// Serialize and deserialize an optional `DateTime<Utc>` as an RFC 3339 string.
pub mod optional_datetime {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

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
            Some(dt) => serializer.serialize_some(&dt.to_rfc3339_opts(SecondsFormat::Secs, true)),
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
            Some(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.with_timezone(&Utc)))
                .map_err(D::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
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
