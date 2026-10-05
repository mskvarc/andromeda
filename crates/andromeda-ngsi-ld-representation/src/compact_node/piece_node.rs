//! A value of JSON text read as a compact node.
//!
//! Every value is a slice of the text it was written in, so reading a node
//! parses only its own level, and a value the model keeps as JSON is admitted
//! straight from its slice.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape, decoded_string};
use andromeda_json_literal::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use serde::de::DeserializeOwned;
use std::{borrow::Cow, convert::Infallible};

impl<'n> CompactNode<'n> for JsonPiece<'n> {
    type Unreadable = Infallible;

    fn shape(self) -> NodeShape<'n, JsonPiece<'n>> {
        // A piece is the text of one JSON value, which always reads as the
        // value its first character announces; the reader cannot fail on it,
        // so its empty answer is never the one taken.
        match self.kind() {
            JsonKind::Null => NodeShape::Null,
            JsonKind::Boolean => NodeShape::Boolean(self.as_bool().unwrap_or_default()),
            JsonKind::Number => NodeShape::Number(Cow::Borrowed(self.text().trim())),
            JsonKind::String => NodeShape::String(self.as_str().unwrap_or(Cow::Borrowed(""))),
            JsonKind::Array => NodeShape::Array(self.elements().unwrap_or_default()),
            JsonKind::Object => NodeShape::Object(NodeMembers::new(self.members().unwrap_or_default())),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, Infallible> {
        Ok(self.to_literal())
    }

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, Infallible> {
        Ok(match self.shape() {
            NodeShape::String(text) => decoded_string(&text),
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
                serde_json::from_str(self.text())
            }
        })
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, Infallible> {
        Ok(serde_json::from_str(self.text()))
    }
}
