//! A JSON-LD value object: one scalar with its datatype or language.
//!
//! Expansion turns every scalar of a Property value into `{"@value": ..}`,
//! optionally typed (`"@type"`, for example `ngsi-ld:DateTime`) or tagged
//! (`"@language"`, `"@direction"`). Numbers stay exact (clause 4.6.3) and strings
//! stay as written (clause 4.6.4).

use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use andromeda_ngsi_ld_number::JsonNumber;
use compact_str::CompactString;
use langtag::LangTagBuf;
use strum::{Display, EnumString, IntoStaticStr};
use thiserror::Error;

/// The scalar of a value object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiteralValue {
    /// An exact JSON number.
    Number(JsonNumber),
    /// A string, as written.
    String(CompactString),
    /// A JSON boolean.
    Boolean(bool),
}

/// The base direction of a string (JSON-LD 1.1 `@direction`).
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum Direction {
    /// Left to right.
    Ltr,
    /// Right to left.
    Rtl,
}

/// Why the parts of a value object do not make one.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LiteralError {
    /// JSON-LD 1.1 section 9.5: a value object carries a datatype or a
    /// language, never both.
    #[error("a value object cannot carry both a datatype and a language")]
    DatatypeBesideLanguage,
    /// JSON-LD 1.1 section 9.5: only a string carries a language or a
    /// direction.
    #[error("only a string value can carry a language or a direction")]
    TaggedNonString,
}

/// One expanded scalar with its optional datatype, language and direction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Literal {
    /// The scalar.
    value: LiteralValue,
    /// The datatype IRI of `@type`.
    datatype: Option<NgsiLdReference>,
    /// The language of `@language`.
    language: Option<LangTagBuf>,
    /// The base direction of `@direction`.
    direction: Option<Direction>,
}

impl Literal {
    /// An untyped, untagged scalar.
    #[must_use]
    pub const fn plain(value: LiteralValue) -> Literal {
        Literal {
            value,
            datatype: None,
            language: None,
            direction: None,
        }
    }

    /// A scalar with a datatype.
    #[must_use]
    pub const fn typed(value: LiteralValue, datatype: NgsiLdReference) -> Literal {
        Literal {
            value,
            datatype: Some(datatype),
            language: None,
            direction: None,
        }
    }

    /// Assemble a value object from its parts.
    ///
    /// # Errors
    ///
    /// Returns [`LiteralError`] for a combination JSON-LD 1.1 section 9.5 does
    /// not admit.
    pub fn try_new(
        value: LiteralValue,
        datatype: Option<NgsiLdReference>,
        language: Option<LangTagBuf>,
        direction: Option<Direction>,
    ) -> Result<Literal, LiteralError> {
        if datatype.is_some() && language.is_some() {
            return Err(LiteralError::DatatypeBesideLanguage);
        }
        let tagged = language.is_some() || direction.is_some();
        match value {
            LiteralValue::Number(_) | LiteralValue::Boolean(_) if tagged => Err(LiteralError::TaggedNonString),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) | LiteralValue::String(_) => Ok(Literal {
                value,
                datatype,
                language,
                direction,
            }),
        }
    }

    /// The same string literal spelled `text`, keeping its datatype, language
    /// and direction; `None` when the literal holds no string.
    #[must_use]
    pub fn respelled(&self, text: CompactString) -> Option<Literal> {
        match self.value {
            LiteralValue::String(_) => Some(Literal {
                value: LiteralValue::String(text),
                // clone: the respelled literal keeps the written one's annotations.
                datatype: self.datatype.clone(),
                language: self.language.clone(),
                direction: self.direction,
            }),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => None,
        }
    }

    /// The scalar.
    #[must_use]
    pub const fn value(&self) -> &LiteralValue {
        &self.value
    }

    /// The datatype IRI, when the value object carries one.
    #[must_use]
    pub const fn datatype(&self) -> Option<&NgsiLdReference> {
        self.datatype.as_ref()
    }

    /// The language, when the value object carries one.
    #[must_use]
    pub const fn language(&self) -> Option<&LangTagBuf> {
        self.language.as_ref()
    }

    /// The base direction, when the value object carries one.
    #[must_use]
    pub const fn direction(&self) -> Option<Direction> {
        self.direction
    }
}

#[cfg(test)]
mod tests {
    use crate::literal::{Direction, Literal, LiteralError, LiteralValue};
    use andromeda_ngsi_ld_number::JsonNumber;

    #[test]
    fn a_language_on_a_number_is_refused() {
        let language = langtag::LangTagBuf::new("en".to_owned()).expect("a language tag");
        pretty_assertions::assert_eq!(
            Literal::try_new(LiteralValue::Number(JsonNumber::from(1_u8)), None, Some(language), None),
            Err(LiteralError::TaggedNonString)
        );
    }

    #[test]
    fn a_direction_renders_in_lower_case() {
        pretty_assertions::assert_eq!(Direction::Rtl.to_string(), "rtl");
    }
}
