//! Reading a boolean query parameter the way clause 6.3.x spells one.

use serde::{Deserialize, Deserializer, de::Error as DeserializeError};

/// Deserialise an optional boolean query parameter case-insensitively.
///
/// Accepts `"true"`, `"TRUE"`, `"True"`, `"false"`, `"FALSE"`, `"False"` and so
/// on. Absent is `None`; anything that is not a boolean is an error rather than
/// a silent `false`, because a client that wrote `local=yes` meant something.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and is not a
/// boolean token.
pub fn deserialize_option_bool_case_insensitive<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    match raw {
        None => Ok(None),
        Some(raw) => match raw.to_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            other => Err(DeserializeError::custom(format!("invalid boolean value: '{other}'"))),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
    use serde::Deserialize;
    use serde_urlencoded::de::Error as UrlDecodeError;

    #[derive(Debug, Deserialize)]
    struct Wrapper {
        /// The parameter under test.
        #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
        value: Option<bool>,
    }

    fn parse(query: &str) -> Result<Wrapper, UrlDecodeError> {
        serde_urlencoded::from_str(query)
    }

    #[test]
    fn an_absent_parameter_is_none() {
        pretty_assertions::assert_eq!(parse("").expect("parses").value, None);
    }

    #[test]
    fn a_boolean_parses_in_any_case() {
        for query in ["value=true", "value=TRUE", "value=True"] {
            pretty_assertions::assert_eq!(parse(query).expect("parses").value, Some(true), "{query}");
        }
        for query in ["value=false", "value=FALSE", "value=False"] {
            pretty_assertions::assert_eq!(parse(query).expect("parses").value, Some(false), "{query}");
        }
    }

    #[test]
    fn anything_that_is_not_a_boolean_is_rejected() {
        for query in ["value=yes", "value=1", "value="] {
            assert!(parse(query).is_err(), "{query}");
        }
    }
}
