//! Serde helper modules for RFC 3339 datetime serialization.
//!
//! Provides `#[serde(with = "...")]` modules for `DateTime<Utc>` fields that
//! need explicit RFC 3339 formatting rather than chrono's default.

/// Serialize/deserialize `DateTime<Utc>` as an RFC 3339 string.
pub mod rfc3339 {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&dt.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// Serialize/deserialize `Option<DateTime<Utc>>` as an optional RFC 3339 string.
pub mod rfc3339_option {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(opt: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(dt) => serializer.serialize_some(&dt.to_rfc3339()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.with_timezone(&Utc)))
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestRequired {
        #[serde(with = "super::rfc3339")]
        ts: chrono::DateTime<Utc>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestOptional {
        #[serde(with = "super::rfc3339_option", skip_serializing_if = "Option::is_none", default)]
        ts: Option<chrono::DateTime<Utc>>,
    }

    #[test]
    fn rfc3339_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestRequired { ts: dt };
        let json = serde_json::to_string(&val).unwrap();
        assert!(json.contains("2024-06-15"));
        let parsed: TestRequired = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.ts, dt);
    }

    #[test]
    fn rfc3339_option_some_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestOptional { ts: Some(dt) };
        let json = serde_json::to_string(&val).unwrap();
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.ts, Some(dt));
    }

    #[test]
    fn rfc3339_option_none_round_trip() {
        let val = TestOptional { ts: None };
        let json = serde_json::to_string(&val).unwrap();
        assert!(!json.contains("ts"));
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.ts, None);
    }
}
