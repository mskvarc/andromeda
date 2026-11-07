//! The NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).

use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_representation::compact_node::CompactNode;
use derive_more::From;
use serde::Serialize;

/// NGSI-LD value family from clause 4.6.3.
///
/// Clause 4.6.4 requires that "implementations shall preserve the
/// representation of the content of the values provided by the context
/// information providers and return the original content when replying to
/// context consumption requests", so this type is the client's own document and
/// carries the value as the JSON it was written as: admitted text, its members
/// in written order, which expansion reads in place.
///
/// A value shaped as a `GeoJSON` geometry is JSON like any other. Clause 5.2.5
/// admits "any JSON value" for a Property, and the clause 4.7.1 geometry rules
/// bind a `GeoProperty` only; a value is read as one only where no `type` is
/// stated (clauses 4.5.2.3 and 4.7.3), which the concise reading decides before
/// a Property value is read.
///
/// What a value *is* under clause 4.6.3 is a separate question from how it is
/// written, and it is decided once, in the lexicon, where the write projection
/// and the clause 4.9 operand parser both read it. No string is sniffed into a
/// typed variant here for that reason: every such type canonicalises through
/// `Display`, so recognising one would rewrite the spelling the client wrote
/// (`URN:NGSI-LD:Building:001`, `2024-01-01T01:00:00+01:00`), which clause
/// 4.6.4 forbids.
#[derive(Debug, Clone, PartialEq, Serialize, From)]
#[serde(transparent)]
pub struct NgsiLdValue(JsonLiteral);

impl NgsiLdValue {
    /// The value a compact node is: the JSON value as written.
    ///
    /// # Errors
    ///
    /// Returns the node's [`CompactNode::Unreadable`] when it has no JSON form;
    /// the inner result refuses JSON no literal admits.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<Result<NgsiLdValue, JsonLiteralError>, N::Unreadable> {
        Ok(node.literal()?.map(NgsiLdValue))
    }

    /// The value as the JSON literal it was written as.
    #[must_use]
    pub const fn literal(&self) -> &JsonLiteral {
        &self.0
    }
}

#[cfg(test)]
mod tests;
