//! A literal decoded from an expanded fixture, read as the data type its
//! `@type` declares, as the entity model reads one.

use crate::queried_entity::value_item::LiteralValue;
use andromeda_ngsi_ld_lexicon::{
    datatype::{declaration::LiteralDeclaration, declared_value::DeclaredValue, primitive_literal::PrimitiveLiteral},
    reference::NgsiLdReference,
    time::value::TimeValue,
};
use andromeda_ngsi_ld_number::{
    JsonNumber,
    declared_number::{DeclaredNumber, NonFinite},
};
use std::borrow::Cow;

/// A literal decoded from an expanded fixture.
#[derive(Debug)]
pub(crate) enum FixtureLiteral {
    /// An exact number.
    Number(JsonNumber),
    /// A string.
    Text(String),
    /// A string carrying its `@language`, as a language map entry does.
    Tagged {
        /// The string.
        text: String,
        /// Its language tag.
        language: String,
    },
    /// A boolean.
    Boolean(bool),
    /// A string whose `@type` declares a clause 4.6.3 time value.
    Time(TimeValue),
    /// A string whose `@type` declares a float or a double beyond the decimals.
    NonFinite(NonFinite),
}

impl FixtureLiteral {
    /// The string `text` read as the data type `datatype` declares.
    pub(crate) fn declared(datatype: Option<&str>, text: &str) -> FixtureLiteral {
        let datatype: Option<NgsiLdReference> = datatype.map(|datatype| datatype.parse().expect("a datatype IRI"));
        match DeclaredValue::of(LiteralDeclaration::new(datatype.as_ref(), None), PrimitiveLiteral::String(text)) {
            None => FixtureLiteral::Text(text.to_owned()),
            Some(declared) => match declared.expect("a value of its declared datatype") {
                DeclaredValue::Time(value) => FixtureLiteral::Time(value),
                DeclaredValue::Number {
                    number: DeclaredNumber::Finite(number),
                    ..
                } => FixtureLiteral::Number(number),
                DeclaredValue::Number {
                    number: DeclaredNumber::NonFinite(value),
                    ..
                } => FixtureLiteral::NonFinite(value),
                DeclaredValue::Boolean(flag) => FixtureLiteral::Boolean(flag),
            },
        }
    }

    /// The literal as the evaluator reads it.
    pub(crate) fn view(&self) -> LiteralValue<'_> {
        match self {
            FixtureLiteral::Number(number) => LiteralValue::Number(Cow::Borrowed(number)),
            FixtureLiteral::Text(text) | FixtureLiteral::Tagged { text, .. } => LiteralValue::Text(Cow::Borrowed(text)),
            FixtureLiteral::Boolean(boolean) => LiteralValue::Boolean(*boolean),
            FixtureLiteral::Time(value) => LiteralValue::Time(*value),
            FixtureLiteral::NonFinite(value) => LiteralValue::NonFiniteNumber(*value),
        }
    }
}
