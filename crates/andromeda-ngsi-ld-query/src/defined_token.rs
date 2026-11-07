//! A query-string value that has to be one of the tokens its parameter defines.
//!
//! A derived enum deserializer refuses an undefined token by quoting it back
//! together with every defined one. Reading the token through its `FromStr`
//! instead refuses it with a message that names the rule and not the value.

use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use std::str::FromStr;
use thiserror::Error;

/// A value that is none of the tokens its parameter defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("not one of the values the parameter defines")]
pub struct UndefinedToken;

/// Deserialize an optional query-string token through its `FromStr`.
///
/// # Errors
///
/// Returns a deserialization error carrying [`UndefinedToken`] when the value
/// does not parse as a `T`.
pub fn deserialize_optional_token<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| value.parse().map_err(|_undefined| DeserializeError::custom(UndefinedToken)))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::defined_token::{UndefinedToken, deserialize_optional_token};
    use serde::Deserialize;
    use strum::EnumString;

    #[derive(Debug, PartialEq, Eq, EnumString)]
    #[strum(serialize_all = "camelCase")]
    enum Mode {
        Inline,
        Flat,
    }

    #[derive(Debug, Deserialize)]
    struct Carrier {
        #[serde(default, deserialize_with = "deserialize_optional_token")]
        mode: Option<Mode>,
    }

    #[test]
    fn a_defined_token_is_read() {
        pretty_assertions::assert_eq!(
            serde_urlencoded::from_str::<Carrier>("mode=flat").map(|carrier| carrier.mode),
            Ok(Some(Mode::Flat))
        );
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<Carrier>("").map(|carrier| carrier.mode), Ok(None));
    }

    #[test]
    fn an_undefined_token_is_refused_without_quoting_it() {
        let message = serde_urlencoded::from_str::<Carrier>("mode=sideways")
            .map(|carrier| carrier.mode)
            .expect_err("undefined")
            .to_string();
        pretty_assertions::assert_eq!(message, UndefinedToken.to_string());
    }
}
