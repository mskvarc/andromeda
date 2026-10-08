//! Admitting one JSON text as a literal.
//!
//! Admission rewrites the text once, level by level from its own slices:
//! whitespace goes, each number takes the canonical spelling
//! [`JsonNumber`] gives it (clause 4.6.3 keeps the mathematical value, not
//! the spelling), each object keeps its members in written order, and an
//! object naming a member twice is refused, because IETF RFC 8259 section 4
//! leaves the meaning of a repeated name undefined and a literal could not
//! give it back.

use crate::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_number::{
    JsonNumber,
    JsonNumberError,
    json_value::{
        READER_NESTING_LIMIT,
        json_pointer::{JsonPointer, JsonPointerSegment},
    },
};
use jstrict::parse::Error as ProcessorParseError;
use serde_json::value::RawValue;
use std::{collections::HashSet, iter::once, str::FromStr};
use thiserror::Error;

/// Why a JSON text is not admitted as a literal.
#[derive(Debug, Error)]
pub enum JsonLiteralError {
    /// The text is not one well-formed JSON value.
    #[error("the text is not one well-formed JSON value: {source}")]
    Syntax {
        /// The JSON grammar error.
        source: serde_json::Error,
    },
    /// An object names one member twice.
    #[error("the member at {path} is named twice in its object, and IETF RFC 8259 gives a repeated member name no meaning")]
    DuplicateMember {
        /// Where the repeated member stands.
        path: JsonPointer,
    },
    /// A number is not one the broker keeps exactly.
    #[error("the JSON number at {path} is refused: {source}")]
    Number {
        /// Where the number stands.
        path: JsonPointer,
        /// The exact-number syntax or supported-range failure.
        source: JsonNumberError,
    },
    /// The value nests deeper than any JSON text the broker reads.
    #[error("the value at {path} nests deeper than the {READER_NESTING_LIMIT} levels a JSON text may have")]
    TooDeep {
        /// The value one level too deep.
        path: JsonPointer,
    },
    /// The literal could not be handed to the JSON-LD processor.
    #[error("the literal could not be read as JSON-LD processor input: {source}")]
    ProcessorText {
        /// The processor's JSON grammar error.
        source: ProcessorParseError,
    },
}

/// What admission refused, before its location is known.
enum Refusal {
    /// The text is not one well-formed JSON value.
    Syntax(serde_json::Error),
    /// An object names one member twice.
    DuplicateMember,
    /// A number is not one the broker keeps exactly.
    Number(JsonNumberError),
    /// The value nests too deep.
    TooDeep,
}

/// A refusal and the path to it, collected leaf first as the walk unwinds.
struct Located {
    /// What was refused.
    refusal: Refusal,
    /// The path from the refused value back up to the root.
    reversed: Vec<JsonPointerSegment>,
}

impl Located {
    /// A refusal at the value being written.
    const fn here(refusal: Refusal) -> Located {
        Located { refusal, reversed: Vec::new() }
    }

    /// The same refusal, one step further from the root.
    fn within(mut self, segment: JsonPointerSegment) -> Located {
        self.reversed.push(segment);
        self
    }
}

impl From<Located> for JsonLiteralError {
    fn from(located: Located) -> JsonLiteralError {
        let path = JsonPointer::from_segments(located.reversed.into_iter().rev().collect());
        match located.refusal {
            Refusal::Syntax(source) => JsonLiteralError::Syntax { source },
            Refusal::DuplicateMember => JsonLiteralError::DuplicateMember { path },
            Refusal::Number(source) => JsonLiteralError::Number { path, source },
            Refusal::TooDeep => JsonLiteralError::TooDeep { path },
        }
    }
}

/// The canonical text of one raw JSON value.
///
/// # Errors
///
/// Returns [`JsonLiteralError`] for a repeated member name, a number outside
/// the supported range, or nesting deeper than [`READER_NESTING_LIMIT`].
pub(crate) fn canonical_text(raw: &RawValue) -> Result<String, JsonLiteralError> {
    let mut written = String::with_capacity(raw.get().len());
    write_canonical(JsonPiece::new(raw), &mut written, 0)?;
    Ok(written)
}

/// Write the canonical text of `piece`, which stands `depth` levels deep.
fn write_canonical(piece: JsonPiece<'_>, written: &mut String, depth: usize) -> Result<(), Located> {
    match piece.kind() {
        JsonKind::Null | JsonKind::Boolean | JsonKind::String => written.push_str(piece.text().trim()),
        JsonKind::Number => {
            let number = JsonNumber::from_str(piece.text().trim()).map_err(|source| Located::here(Refusal::Number(source)))?;
            written.push_str(&number.to_string());
        }
        JsonKind::Array => {
            let elements = nested(|| piece.array_elements(), depth)?;
            written.push('[');
            for (index, element) in elements.into_iter().enumerate() {
                if index > 0 {
                    written.push(',');
                }
                write_canonical(element, written, depth + 1).map_err(|located| located.within(JsonPointerSegment::Index(index)))?;
            }
            written.push(']');
        }
        JsonKind::Object => {
            let members = nested(|| piece.object_members(), depth)?;
            let mut seen = HashSet::with_capacity(members.len());
            written.push('{');
            for (index, (name, value)) in members.iter().enumerate() {
                let within = || JsonPointerSegment::Member(Box::from(name.as_ref()));
                if !seen.insert(name.as_ref()) {
                    return Err(Located::here(Refusal::DuplicateMember).within(within()));
                }
                if index > 0 {
                    written.push(',');
                }
                let quoted = serde_json::to_string(name.as_ref()).map_err(|source| Located::here(Refusal::Syntax(source)).within(within()))?;
                written.extend(once(quoted.as_str()).chain(once(":")));
                write_canonical(*value, written, depth + 1).map_err(|located| located.within(within()))?;
            }
            written.push('}');
        }
    }
    Ok(())
}

/// The children of a container, refused when the container already stands at
/// the deepest level a JSON text may have.
fn nested<T>(children: impl FnOnce() -> Result<T, serde_json::Error>, depth: usize) -> Result<T, Located> {
    if depth >= READER_NESTING_LIMIT {
        return Err(Located::here(Refusal::TooDeep));
    }
    children().map_err(|source| Located::here(Refusal::Syntax(source)))
}
