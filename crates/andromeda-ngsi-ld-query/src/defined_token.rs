//! A query-string value that has to be one of the tokens its parameter defines.
//!
//! A derived enum deserializer or a derived `FromStr` refuses an undefined
//! token in its own words ("Matching variant not found"). Reading the token
//! through [`parse_token`] instead refuses it by naming the value written and
//! the tokens the parameter takes, so the client sees what to write instead.

use derive_more::{Display, From};
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use std::str::FromStr;
use strum::VariantNames;
use thiserror::Error;

/// A word a client wrote where a token or a member name stands, as written.
///
/// It is written back quoted and escaped, so a refusal naming it stays on one
/// line whatever it holds.
#[derive(Debug, Clone, PartialEq, Eq, Display, From)]
#[display("'{}'", _0.escape_debug())]
pub struct WrittenWord(String);

/// The tokens or member names a declaration takes, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[display("{}", listed(_0))]
pub struct Vocabulary(pub &'static [&'static str]);

/// The words `words` lists, separated by commas.
fn listed(words: &[&str]) -> String {
    match words {
        [] => "none".to_owned(),
        words => words.join(", "),
    }
}

/// A value that is none of the tokens its parameter defines.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{given} is not one of {takes}")]
pub struct UndefinedToken {
    /// The value as written.
    pub given: WrittenWord,
    /// The tokens the parameter takes.
    pub takes: Vocabulary,
}

/// Read `written` as one of the tokens of `T`.
///
/// # Errors
///
/// Returns [`UndefinedToken`] when `written` is none of them.
pub fn parse_token<T: FromStr + VariantNames>(written: &str) -> Result<T, UndefinedToken> {
    written.parse().map_err(|_undefined| UndefinedToken {
        given: WrittenWord::from(written.to_owned()),
        takes: Vocabulary(T::VARIANTS),
    })
}

/// Deserialize an optional query-string token through [`parse_token`].
///
/// # Errors
///
/// Returns a deserialization error carrying [`UndefinedToken`] when the value
/// is none of the tokens of `T`.
pub fn deserialize_optional_token<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + VariantNames,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| parse_token(&value).map_err(DeserializeError::custom)).transpose()
}

#[cfg(test)]
mod tests {
    use crate::defined_token::{UndefinedToken, Vocabulary, WrittenWord, deserialize_optional_token, parse_token};
    use serde::Deserialize;
    use strum::{EnumString, VariantNames};

    #[derive(Debug, PartialEq, Eq, EnumString, VariantNames)]
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
    fn an_undefined_token_names_the_value_and_the_tokens_taken() {
        pretty_assertions::assert_eq!(
            parse_token::<Mode>("sideways"),
            Err(UndefinedToken {
                given: WrittenWord::from("sideways".to_owned()),
                takes: Vocabulary(&["inline", "flat"]),
            })
        );
        let message = serde_urlencoded::from_str::<Carrier>("mode=sideways")
            .map(|carrier| carrier.mode)
            .expect_err("undefined")
            .to_string();
        pretty_assertions::assert_eq!(message, "'sideways' is not one of inline, flat");
    }

    /// A value is written back on one line, whatever it holds.
    #[test]
    fn the_value_written_is_escaped_onto_one_line() {
        let refusal = parse_token::<Mode>("a\nb").expect_err("undefined");
        pretty_assertions::assert_eq!(refusal.to_string(), r"'a\nb' is not one of inline, flat");
        pretty_assertions::assert_eq!(Mode::VARIANTS, ["inline", "flat"]);
    }
}
