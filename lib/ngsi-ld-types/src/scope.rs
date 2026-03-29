use crate::error::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::LazyLock;

static SCOPE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(/?([A-Za-z][A-Za-z0-9_]*)(/([A-Za-z][A-Za-z0-9_]*))*|urn:ngsi-ld:null)$").unwrap());

/// A validated NGSI-LD scope string (e.g. `/Madrid/Gardens`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeBuf(String);

impl ScopeBuf {
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if SCOPE_REGEX.is_match(&s) { Ok(Self(s)) } else { Err(Error::InvalidScope(s)) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for ScopeBuf {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl std::fmt::Display for ScopeBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ScopeBuf {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ScopeBuf {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// An NGSI-LD scope — either a single scope string or a list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdScope {
    Single(ScopeBuf),
    List(Vec<ScopeBuf>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_simple_scope() {
        assert!(ScopeBuf::new("/Madrid").is_ok());
    }

    #[test]
    fn valid_nested_scope() {
        assert!(ScopeBuf::new("/Madrid/Gardens/ParqueNorte").is_ok());
    }

    #[test]
    fn valid_null_scope() {
        assert!(ScopeBuf::new("urn:ngsi-ld:null").is_ok());
    }

    #[test]
    fn valid_scope_without_leading_slash() {
        assert!(ScopeBuf::new("Madrid").is_ok());
    }

    #[test]
    fn invalid_scope_with_spaces() {
        assert!(ScopeBuf::new("/Madrid Gardens").is_err());
    }

    #[test]
    fn scope_single_round_trips() {
        let scope = NgsiLdScope::Single(ScopeBuf::new("/Madrid").unwrap());
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "\"/Madrid\"");
        let parsed: NgsiLdScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn scope_list_round_trips() {
        let scope = NgsiLdScope::List(vec![ScopeBuf::new("/Madrid").unwrap(), ScopeBuf::new("/Barcelona").unwrap()]);
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: NgsiLdScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }
}
