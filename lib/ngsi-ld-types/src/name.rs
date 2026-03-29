use crate::error::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::LazyLock;

static NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{L}[\p{L}\p{N}_]*(?::\p{L}[\p{L}\p{N}_]*)?$").unwrap());

/// A validated NGSI-LD attribute or type name.
///
/// Names must match the pattern: a Unicode letter followed by letters, digits,
/// or underscores, optionally followed by a colon and another such segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NameBuf(String);

impl NameBuf {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if NAME_REGEX.is_match(&s) {
            Ok(Self(s))
        } else {
            Err(Error::InvalidAttributeName(s))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for NameBuf {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl std::fmt::Display for NameBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for NameBuf {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NameBuf {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_simple_name() {
        assert!(NameBuf::new("temperature").is_ok());
    }

    #[test]
    fn valid_name_with_digits() {
        assert!(NameBuf::new("sensor42").is_ok());
    }

    #[test]
    fn valid_name_with_underscore() {
        assert!(NameBuf::new("air_quality").is_ok());
    }

    #[test]
    fn valid_prefixed_name() {
        assert!(NameBuf::new("schema:name").is_ok());
    }

    #[test]
    fn invalid_starts_with_digit() {
        assert!(NameBuf::new("42sensor").is_err());
    }

    #[test]
    fn invalid_starts_with_underscore() {
        assert!(NameBuf::new("_private").is_err());
    }

    #[test]
    fn invalid_empty_string() {
        assert!(NameBuf::new("").is_err());
    }

    #[test]
    fn round_trips_through_json() {
        let name = NameBuf::new("temperature").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"temperature\"");
        let parsed: NameBuf = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, name);
    }
}
