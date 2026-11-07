//! A JSON-LD value object: one scalar with its datatype or language.
//!
//! Expansion turns every scalar of a Property value into `{"@value": ..}`,
//! optionally typed (`"@type"`, for example `ngsi-ld:DateTime`) or tagged
//! (`"@language"`, `"@direction"`). Numbers stay exact (clause 4.6.3), a string
//! declared as no time value stays as written (clause 4.6.4), and one declared
//! as a time value takes its one served spelling when a write commits it.

use andromeda_ngsi_ld_lexicon::{datatype::primitive_literal::PrimitiveLiteral, reference::NgsiLdReference};
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

impl LiteralValue {
    /// The scalar as the datatype checks read it.
    #[must_use]
    pub fn primitive(&self) -> PrimitiveLiteral<'_> {
        match self {
            LiteralValue::Number(number) => PrimitiveLiteral::Number(number),
            LiteralValue::String(text) => PrimitiveLiteral::String(text),
            LiteralValue::Boolean(flag) => PrimitiveLiteral::Boolean(*flag),
        }
    }
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
    /// JSON-LD 1.1 Processing Algorithms and API section 5.1.2 step 15.1: a
    /// value object carrying a datatype carries no language and no direction.
    #[error("a value object cannot carry a datatype beside a language or a direction")]
    DatatypeBesideTag,
    /// JSON-LD 1.1 section 9.5: only a string carries a language or a
    /// direction.
    #[error("only a string value can carry a language or a direction")]
    TaggedNonString,
}

/// One expanded scalar with its optional datatype, language and direction.
///
/// Most scalars carry none of the three, so they are held apart from the
/// scalar and only when written: a value array of numbers costs the scalars
/// alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Literal {
    /// The scalar.
    value: LiteralValue,
    /// The datatype, or the language and direction, the value object carries.
    annotation: Option<Box<Annotation>>,
}

/// What a value object states beside its scalar: JSON-LD 1.1 section 9.5
/// gives it a datatype or a language and direction, never both.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Annotation {
    /// The datatype IRI of `@type`.
    Datatype(NgsiLdReference),
    /// The `@language` and `@direction` of a string, at least one of them.
    Tagged {
        /// The language of `@language`.
        language: Option<LangTagBuf>,
        /// The base direction of `@direction`.
        direction: Option<Direction>,
    },
}

impl Literal {
    /// An untyped, untagged scalar.
    #[must_use]
    pub const fn plain(value: LiteralValue) -> Literal {
        Literal { value, annotation: None }
    }

    /// A scalar with a datatype.
    #[must_use]
    pub fn typed(value: LiteralValue, datatype: NgsiLdReference) -> Literal {
        Literal {
            value,
            annotation: Some(Box::new(Annotation::Datatype(datatype))),
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
        let tagged = language.is_some() || direction.is_some();
        let annotation = match (datatype, tagged) {
            (Some(_), true) => return Err(LiteralError::DatatypeBesideTag),
            (Some(datatype), false) => Some(Box::new(Annotation::Datatype(datatype))),
            (None, true) => Some(Box::new(Annotation::Tagged { language, direction })),
            (None, false) => None,
        };
        match value {
            LiteralValue::Number(_) | LiteralValue::Boolean(_) if tagged => Err(LiteralError::TaggedNonString),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) | LiteralValue::String(_) => Ok(Literal { value, annotation }),
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
                annotation: self.annotation.clone(),
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
    pub fn datatype(&self) -> Option<&NgsiLdReference> {
        match self.annotation.as_deref()? {
            Annotation::Datatype(datatype) => Some(datatype),
            Annotation::Tagged { .. } => None,
        }
    }

    /// The language, when the value object carries one.
    #[must_use]
    pub fn language(&self) -> Option<&LangTagBuf> {
        match self.annotation.as_deref()? {
            Annotation::Tagged { language, .. } => language.as_ref(),
            Annotation::Datatype(_) => None,
        }
    }

    /// The base direction, when the value object carries one.
    #[must_use]
    pub fn direction(&self) -> Option<Direction> {
        match self.annotation.as_deref()? {
            Annotation::Tagged { direction, .. } => *direction,
            Annotation::Datatype(_) => None,
        }
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
