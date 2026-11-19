//! A Boolean query-string parameter (`local`, `count`, `entityMap` and the
//! like).
//!
//! These parameters are Booleans, written `true` or `false`. Read through the
//! standard Boolean parser, any other word is refused in that parser's own
//! words; read here, it is refused naming the two values it takes.

use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use thiserror::Error;

/// A value that is neither `true` nor `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("not true or false")]
pub struct NotAFlag;

/// Read `written` as a Boolean.
///
/// # Errors
///
/// Returns [`NotAFlag`] when `written` is neither `true` nor `false`.
pub fn parse_flag(written: &str) -> Result<bool, NotAFlag> {
    match written {
        "true" => Ok(true),
        "false" => Ok(false),
        _other => Err(NotAFlag),
    }
}

/// Deserialize an optional Boolean query-string parameter.
///
/// # Errors
///
/// Returns a deserialization error carrying [`NotAFlag`] when the value is
/// neither `true` nor `false`.
pub fn deserialize_optional_flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| parse_flag(&value).map_err(DeserializeError::custom)).transpose()
}

/// Deserialize a Boolean query-string parameter that reads as `false` when
/// absent, used with `#[serde(default)]`.
///
/// # Errors
///
/// As [`deserialize_optional_flag`].
pub fn deserialize_flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let raw = String::deserialize(deserializer)?;
    parse_flag(&raw).map_err(DeserializeError::custom)
}

#[cfg(test)]
mod tests {
    use crate::query_flag::{NotAFlag, deserialize_flag, deserialize_optional_flag, parse_flag};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Carrier {
        #[serde(default, deserialize_with = "deserialize_optional_flag")]
        local: Option<bool>,
        #[serde(default, deserialize_with = "deserialize_flag")]
        details: bool,
    }

    #[test]
    fn true_and_false_are_read_and_absent_is_none() {
        for (query, local, details) in [
            ("local=true&details=true", Some(true), true),
            ("local=false", Some(false), false),
            ("", None, false),
        ] {
            let read = serde_urlencoded::from_str::<Carrier>(query).expect(query);
            pretty_assertions::assert_eq!((read.local, read.details), (local, details), "{query}");
        }
    }

    #[test]
    fn any_other_word_is_refused_naming_the_two_values() {
        for written in ["yes", "True", "1", ""] {
            pretty_assertions::assert_eq!(parse_flag(written), Err(NotAFlag), "{written}");
        }
        let refusal = serde_urlencoded::from_str::<Carrier>("local=yes").expect_err("not a Boolean");
        pretty_assertions::assert_eq!(refusal.to_string(), "not true or false");
    }
}
