//! A value of JSON text read as a compact node whose geometries are read no
//! further than a position limit.
//!
//! A piece reads a geometry object into its coordinates whatever their number,
//! so a request carrying a geometry of millions of positions would cost the
//! broker the whole geometry before any budget on positions could refuse it.
//! This node hands every geometry object to the geometry reader with the limit,
//! which stops recording at the first ordinate no geometry within the limit
//! can hold (ETSI GS CIM 009 v1.9.1 clause 6.3.2), so the cost of refusing a
//! geometry does not follow its size.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, piece::JsonPiece};
use andromeda_ngsi_ld_geometry::{geometry_census::GeometryPositionLimit, written_geometry::WrittenGeometry};
use serde::de::DeserializeOwned;
use std::convert::Infallible;

/// A piece of JSON text whose geometries are read within a position limit.
#[derive(Debug, Clone, Copy)]
pub struct BoundedPiece<'n> {
    /// The text of the value.
    piece: JsonPiece<'n>,
    /// The positions a geometry in the value may hold.
    positions: GeometryPositionLimit,
}

impl<'n> BoundedPiece<'n> {
    /// Read `piece`, a geometry of it held to `positions`.
    #[must_use]
    pub const fn new(piece: JsonPiece<'n>, positions: GeometryPositionLimit) -> BoundedPiece<'n> {
        BoundedPiece { piece, positions }
    }

    /// A value nested in this one, bounded the same way.
    const fn nested(self, piece: JsonPiece<'n>) -> BoundedPiece<'n> {
        BoundedPiece {
            piece,
            positions: self.positions,
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

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, Infallible> {
        self.piece.decoded()
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
    use crate::compact_node::{CompactNode, bounded_piece::BoundedPiece};
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use serde_json::{json, value::RawValue};
    use std::num::NonZeroUsize;

    fn limit(positions: usize) -> GeometryPositionLimit {
        GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit"))
    }

    fn square() -> Box<RawValue> {
        RawValue::from_string(json!({"value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}).to_string()).expect("JSON")
    }

    #[test]
    fn a_geometry_within_the_limit_reads_as_its_geometry() {
        let text = square();
        let piece = BoundedPiece::new(JsonPiece::new(&text), limit(5));
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
        let piece = BoundedPiece::new(JsonPiece::new(&text), limit(1));
        let written = piece
            .member("value")
            .expect("the member")
            .read_geometry()
            .expect("readable")
            .expect("a geometry");
        pretty_assertions::assert_eq!(written.exceeded_positions(), Some(limit(1)));
    }
}
