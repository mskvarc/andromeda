//! The NGSI-LD Null inside a literal (clauses 4.5.0, 4.5.24.2 and 5.5.4).
//!
//! Inside a literal the NGSI-LD Null is the string `urn:ngsi-ld:null`, never a
//! JSON `null`: expansion never descends into an `@json` value, so nothing
//! there was ever expanded into the IRI spelling, and a JSON `null` survives
//! expansion untouched as ordinary data.

use crate::{
    JsonLiteral,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_vocabulary::NULL_URI;

impl JsonLiteral {
    /// Whether the literal is itself the NGSI-LD Null: the deletion encoding
    /// of a `JsonProperty` (clause 4.5.24.2).
    #[must_use]
    pub fn is_null_sentinel(&self) -> bool {
        is_null_string(self.piece())
    }

    /// Whether some member of the literal, at any depth and through arrays,
    /// holds the NGSI-LD Null.
    ///
    /// That is the clause 5.5.4 third prohibition, which only a merge fragment
    /// is exempt from: there the member is deleted (clause 5.5.12, IETF RFC
    /// 7396). The literal being the Null itself is the second prohibition, not
    /// this one.
    #[must_use]
    pub fn carries_null_sentinel_under_a_key(&self) -> bool {
        carries_under_a_member(self.piece())
    }
}

/// Whether `piece` is the NGSI-LD Null string.
pub(crate) fn is_null_string(piece: JsonPiece<'_>) -> bool {
    piece.as_str().is_some_and(|text| text == NULL_URI.as_str())
}

/// Whether some member below `piece` holds the NGSI-LD Null string.
fn carries_under_a_member(piece: JsonPiece<'_>) -> bool {
    match piece.kind() {
        JsonKind::Object => piece
            .members()
            .is_some_and(|members| members.into_iter().any(|(_, value)| is_or_contains_null_string(value))),
        JsonKind::Array => piece.elements().is_some_and(|elements| elements.into_iter().any(carries_under_a_member)),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => false,
    }
}

/// Whether a value held under some member is the NGSI-LD Null string, or
/// nests one.
fn is_or_contains_null_string(piece: JsonPiece<'_>) -> bool {
    match piece.kind() {
        JsonKind::String => is_null_string(piece),
        JsonKind::Array => piece.elements().is_some_and(|elements| elements.into_iter().any(is_or_contains_null_string)),
        JsonKind::Object => piece
            .members()
            .is_some_and(|members| members.into_iter().any(|(_, value)| is_or_contains_null_string(value))),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number => false,
    }
}
