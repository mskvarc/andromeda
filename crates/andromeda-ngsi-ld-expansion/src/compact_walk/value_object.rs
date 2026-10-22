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
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::keyword;
use jsonld::{Direction as JsonLdDirection, Id, Term, ValidId, syntax::Keyword};

/// Expand the entries of a value object.
///
/// # Errors
///
/// Declines every combination the algorithm refuses, and `@index`.
pub fn expand_value_object<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, entries: &[KeyedEntry<'_, N>]) -> Result<Expanded, Decline> {
    let mut value = None;
    let mut datatype = None;
    let mut json = false;
    let mut language = None;
    let mut direction = None;
    for entry in entries {
        match entry.term.as_ref() {
            Term::Keyword(Keyword::Value) => value = Some(entry.value),
            Term::Keyword(Keyword::Type) => {
                let type_name = entry.value.text().ok_or(Decline::MalformedJsonLd)?;
                match scope.expand_vocab(&type_name)?.as_ref() {
                    Term::Keyword(Keyword::Json) => json = true,
                    Term::Id(Id::Valid(ValidId::Iri(iri))) => datatype = Some(iri.as_str().to_owned()),
                    Term::Null | Term::Id(Id::Valid(ValidId::Blank(_)) | Id::Invalid(_)) | Term::Keyword(_) => return Err(Decline::MalformedJsonLd),
                }
            }
            Term::Keyword(Keyword::Language) => {
                let tag = entry.value.text().ok_or(Decline::MalformedJsonLd)?;
                if tag != keyword::NONE {
                    language = Some(tag.into_owned());
                }
            }
            Term::Keyword(Keyword::Direction) => {
                let base = entry
                    .value
                    .text()
                    .and_then(|text| JsonLdDirection::try_from(text.as_ref()).ok())
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
        return Ok(Expanded::One(ExpandedItem::Json(json_literal(value)?)));
    }
    let shape = value.shape();
    if matches!(shape, NodeShape::Null) {
        return Ok(Expanded::Null);
    }
    let is_string = matches!(shape, NodeShape::String(_));
    let scalar = scalar(shape)?;
    let tagged = language.is_some() || direction.is_some();
    if tagged && (datatype.is_some() || !is_string) {
        return Err(Decline::MalformedJsonLd);
    }
    Ok(Expanded::One(ExpandedItem::Value(ValueObject {
        scalar,
        datatype,
        language,
        direction,
    })))
}

/// The JSON literal a written value is (JSON-LD 1.1 expansion, step 13.7 and
/// the value object of step 13.4.7.1): the value as written, admitted.
///
/// # Errors
///
/// Declines a value the literal does not admit, which the processor then
/// refuses with the precise error.
pub(crate) fn json_literal<'n, N: CompactNode<'n>>(value: N) -> Result<JsonLiteral, Decline> {
    value
        .literal()
        .map_err(|_| Decline::CompactSerialization)?
        .map_err(|_| Decline::MalformedJsonLd)
}
