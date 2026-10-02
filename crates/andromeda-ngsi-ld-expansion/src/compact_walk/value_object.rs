//! Expanding a value object, an object written with `@value` (JSON-LD 1.1
//! expansion algorithm, steps 13.4.7 and 15).

use crate::{
    compact_walk::{
        element::Expanded,
        literal::{direction_of, scalar},
        object::KeyedEntry,
    },
    decline::Decline,
    expanded_tree::{ExpandedItem, ValueObject},
    term_scope::TermScope,
};
use jsonld::{Direction as JsonLdDirection, Id, Term, ValidId, syntax::Keyword};
use serde_json::Value;

/// Expand the entries of a value object.
///
/// # Errors
///
/// Declines every combination the algorithm refuses, and `@index`.
pub fn expand_value_object(scope: &TermScope<'_>, entries: &[KeyedEntry<'_>]) -> Result<Expanded, Decline> {
    let mut value = None;
    let mut datatype = None;
    let mut json = false;
    let mut language = None;
    let mut direction = None;
    for entry in entries {
        match entry.term.as_ref() {
            Term::Keyword(Keyword::Value) => value = Some(entry.value),
            Term::Keyword(Keyword::Type) => {
                let type_name = entry.value.as_str().ok_or(Decline::MalformedJsonLd)?;
                match scope.expand_vocab(type_name)?.as_ref() {
                    Term::Keyword(Keyword::Json) => json = true,
                    Term::Id(Id::Valid(ValidId::Iri(iri))) => datatype = Some(iri.as_str().to_owned()),
                    Term::Null | Term::Id(Id::Valid(ValidId::Blank(_)) | Id::Invalid(_)) | Term::Keyword(_) => return Err(Decline::MalformedJsonLd),
                }
            }
            Term::Keyword(Keyword::Language) => {
                let tag = entry.value.as_str().ok_or(Decline::MalformedJsonLd)?;
                if tag != "@none" {
                    language = Some(tag.to_owned());
                }
            }
            Term::Keyword(Keyword::Direction) => {
                let base = entry
                    .value
                    .as_str()
                    .and_then(|text| JsonLdDirection::try_from(text).ok())
                    .ok_or(Decline::MalformedJsonLd)?;
                direction = Some(direction_of(base));
            }
            Term::Keyword(Keyword::Index) => return Err(Decline::UnsupportedKeyword),
            Term::Null | Term::Id(_) | Term::Keyword(_) => return Err(Decline::MalformedJsonLd),
        }
    }
    let value = value.ok_or(Decline::MalformedJsonLd)?;
    if json {
        if language.is_some() || direction.is_some() {
            return Err(Decline::MalformedJsonLd);
        }
        return Ok(Expanded::One(ExpandedItem::Json(value.clone()))); // clone: a JSON literal carries the written value verbatim
    }
    if value.is_null() {
        return Ok(Expanded::Null);
    }
    let scalar = scalar(value)?;
    let tagged = language.is_some() || direction.is_some();
    if tagged && (datatype.is_some() || !matches!(value, Value::String(_))) {
        return Err(Decline::MalformedJsonLd);
    }
    Ok(Expanded::One(ExpandedItem::Value(ValueObject {
        scalar,
        datatype,
        language,
        direction,
    })))
}
