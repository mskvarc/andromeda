//! A value of JSON text read as a compact node whose geometries and growing
//! members are read no further than the entity limits.
//!
//! A piece reads a geometry object into its coordinates whatever their number,
//! so a request carrying a geometry of millions of positions would cost the
//! broker the whole geometry before any budget on positions could refuse it.
//! This node hands every geometry object to the geometry reader with the limit,
//! which stops recording at the first ordinate no geometry within the limit
//! can hold (ETSI GS CIM 009 v1.9.1 clause 6.3.2), so the cost of refusing a
//! geometry does not follow its size. It also carries the budget of values the
//! members that grow without bound are recorded against, so an entity over the
//! entity value limit costs what the limit costs whatever its size.

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    value_budget::ValueBudget,
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, piece::JsonPiece};
use andromeda_ngsi_ld_geometry::{geometry_census::GeometryPositionLimit, written_geometry::WrittenGeometry};
use serde::de::DeserializeSeed;
use std::convert::Infallible;

/// A piece of JSON text whose geometries are read within a position limit and
/// whose growing members are recorded against a budget of values.
#[derive(Debug, Clone, Copy)]
pub struct BoundedPiece<'n> {
    /// The text of the value.
    piece: JsonPiece<'n>,
    /// The positions a geometry in the value may hold.
    positions: GeometryPositionLimit,
    /// The values of the entity that may still be recorded.
    values: &'n ValueBudget,
}

impl<'n> BoundedPiece<'n> {
    /// Read `piece`, a geometry of it held to `positions` and its growing
    /// members recorded against `values`.
    #[must_use]
    pub const fn new(piece: JsonPiece<'n>, positions: GeometryPositionLimit, values: &'n ValueBudget) -> BoundedPiece<'n> {
        BoundedPiece { piece, positions, values }
    }

    /// A value nested in this one, bounded the same way.
    const fn nested(self, piece: JsonPiece<'n>) -> BoundedPiece<'n> {
        BoundedPiece {
            piece,
            positions: self.positions,
            values: self.values,
        }
    }
}

impl<'n> CompactNode<'n> for BoundedPiece<'n> {
    type Unreadable = Infallible;

    fn shape(self) -> NodeShape<'n, BoundedPiece<'n>> {
        match self.piece.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(text),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(|item| self.nested(item)).collect()),
            NodeShape::Object(members) => NodeShape::Object(NodeMembers::new(members.into_iter().map(|(name, item)| (name, self.nested(item))).collect())),
            NodeShape::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, Infallible> {
        self.piece.literal()
    }

    fn decoded_by<T, S: for<'de> DeserializeSeed<'de, Value = T>>(self, seed: S) -> Result<Result<T, serde_json::Error>, Infallible> {
        self.piece.decoded_by(seed)
    }

    fn is_null(self) -> bool {
        self.piece.is_null()
    }

    fn value_budget(self) -> Option<&'n ValueBudget> {
        Some(self.values)
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, Infallible> {
        Ok(WrittenGeometry::read_within(self.piece.text(), self.positions))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        compact_node::{CompactNode, bounded_piece::BoundedPiece},
        value_budget::ValueBudget,
    };
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::{json, value::RawValue};
    use std::num::NonZeroUsize;

    fn values() -> ValueBudget {
        ValueBudget::new(JsonValueLimit::new(NonZeroUsize::new(1_000_000).expect("a positive limit")))
    }

    fn limit(positions: usize) -> GeometryPositionLimit {
        GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit"))
    }

    fn square() -> Box<RawValue> {
        RawValue::from_string(json!({"value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}).to_string()).expect("JSON")
    }

    #[test]
    fn a_geometry_within_the_limit_reads_as_its_geometry() {
        let text = square();
        let budget = values();
        let piece = BoundedPiece::new(JsonPiece::new(&text), limit(5), &budget);
        let written = piece
            .member("value")
            .expect("the member")
            .read_geometry()
            .expect("readable")
            .expect("a geometry");
        assert!(written.exceeded_positions().is_none());
        written.into_geometry().expect("a valid polygon");
    }

    #[test]
    fn a_geometry_past_the_limit_reads_as_exceeding_it() {
        let text = square();
        let budget = values();
        let piece = BoundedPiece::new(JsonPiece::new(&text), limit(1), &budget);
        let written = piece
            .member("value")
            .expect("the member")
            .read_geometry()
            .expect("readable")
            .expect("a geometry");
        pretty_assertions::assert_eq!(written.exceeded_positions(), Some(limit(1)));
    }
}
