//! The NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).

use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geojson::GeometryError, geometry::Geometry};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use serde::{Serialize, Serializer};
use thiserror::Error;

/// The member a geometry names its kind in.
const TYPE_MEMBER: &str = "type";

/// The member a geometry carries its positions in.
const COORDINATES_MEMBER: &str = "coordinates";

/// NGSI-LD value family from clause 4.6.3.
///
/// Clause 4.6.4 requires that "implementations shall preserve the
/// representation of the content of the values provided by the context
/// information providers and return the original content when replying to
/// context consumption requests", so this type is the client's own document and
/// carries the value as the JSON it was written as: admitted text, its members
/// in written order, which expansion reads in place. The one value that is not
/// carried as JSON is a `GeoJSON` geometry, and that is not a re-spelling:
/// clause 4.7 adopts IETF RFC 7946 whole and a geometry that is not one is a
/// clause 5.5.4 `BadRequestData` the decode has to be able to raise.
///
/// What a value *is* under clause 4.6.3 is a separate question from how it is
/// written, and it is decided once, in the lexicon, where the write projection
/// and the clause 4.9 operand parser both read it. No string is sniffed into a
/// typed variant here for that reason: every such type canonicalises through
/// `Display`, so recognising one would rewrite the spelling the client wrote
/// (`URN:NGSI-LD:Building:001`, `2024-01-01T01:00:00+01:00`), which clause
/// 4.6.4 forbids.
#[derive(Debug, Clone, PartialEq)]
pub enum NgsiLdValue {
    /// A JSON value, as written.
    Json(JsonLiteral),
    /// A `GeoJSON` geometry value (clause 4.7).
    Geometry(Geometry),
}

/// Why a compact node is no value.
#[derive(Debug, Error)]
pub enum ValueRefusal {
    /// The node carries `type` and `coordinates` and is no `GeoJSON`
    /// geometry: a value shaped as one is a geometry or an error, never
    /// demoted to plain JSON.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// The node carries `type` and `coordinates` and is no `GeoJSON`
    /// geometry object the reader reads.
    #[error("the GeoJSON-shaped value is no geometry object: {0}")]
    Written(#[source] serde_json::Error),
    /// The JSON is no value the broker admits.
    #[error(transparent)]
    Literal(#[from] JsonLiteralError),
}

// Hand-written: the wire form is the JSON value itself, whichever variant
// holds it, which `#[serde(untagged)]` writes only by buffering.
impl Serialize for NgsiLdValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdValue::Json(value) => value.serialize(serializer),
            NgsiLdValue::Geometry(value) => value.serialize(serializer),
        }
    }
}

impl NgsiLdValue {
    /// The value a compact node is: a geometry when it carries both `type`
    /// and `coordinates`, or is held as a typed geometry, the JSON value as
    /// written otherwise.
    ///
    /// # Errors
    ///
    /// Returns the node's [`CompactNode::Unreadable`] when it has no JSON form;
    /// the inner result refuses a `GeoJSON`-shaped value that is no geometry
    /// and JSON no literal admits.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<Result<NgsiLdValue, ValueRefusal>, N::Unreadable> {
        match node.shape() {
            NodeShape::Geometry(form) => Ok(Ok(NgsiLdValue::Geometry(typed(form)))),
            NodeShape::Object(members) if is_geojson_shaped(&members) => Ok(node
                .read_geometry()?
                .map_err(ValueRefusal::Written)
                .and_then(|written| written.into_geometry().map_err(ValueRefusal::from))
                .map(NgsiLdValue::Geometry)),
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) => {
                Ok(node.literal()?.map(NgsiLdValue::Json).map_err(ValueRefusal::from))
            }
        }
    }

    /// The value as one JSON literal: the literal it holds, or the geometry
    /// written out.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLiteralError`] of a geometry whose text no literal
    /// admits, which a typed geometry never is.
    pub fn to_literal(&self) -> Result<JsonLiteral, JsonLiteralError> {
        match self {
            // clone: the caller owns the literal it asked for
            NgsiLdValue::Json(literal) => Ok(literal.clone()),
            NgsiLdValue::Geometry(geometry) => serde_json::to_string(geometry)
                .map_err(|source| JsonLiteralError::Syntax { source })
                .and_then(|text| JsonLiteral::admit(&text)),
        }
    }
}

/// Whether an object carries both members a `GeoJSON` geometry is made of.
fn is_geojson_shaped<'n, N: CompactNode<'n>>(members: &NodeMembers<'n, N>) -> bool {
    members.contains(TYPE_MEMBER) && members.contains(COORDINATES_MEMBER)
}

/// The geometry a node holds typed.
fn typed(form: &GeometryForm) -> Geometry {
    // clone: the compact value owns its geometry, the tree keeps the one it was written from
    Geometry::new(form.clone())
}

#[cfg(test)]
mod tests;
