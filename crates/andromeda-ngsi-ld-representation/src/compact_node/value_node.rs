//! A parsed JSON value read as a compact node.
//!
//! The temporal write path and the document renderers still hand the rules a
//! parsed document; they read it through the same view as text.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape, decoded_string};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{borrow::Cow, convert::Infallible};

impl<'n> CompactNode<'n> for &'n Value {
    type Unreadable = Infallible;

    fn shape(self) -> NodeShape<'n, &'n Value> {
        match self {
            Value::Null => NodeShape::Null,
            Value::Bool(flag) => NodeShape::Boolean(*flag),
            Value::Number(number) => NodeShape::Number(Cow::Owned(number.to_string())),
            Value::String(text) => NodeShape::String(Cow::Borrowed(text)),
            Value::Array(items) => NodeShape::Array(items.iter().collect()),
            Value::Object(members) => NodeShape::Object(object_members(members)),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, Infallible> {
        Ok(JsonLiteral::admit(&self.to_string()))
    }

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, Infallible> {
        Ok(match self {
            Value::String(text) => decoded_string(text),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => deserialize_exact(self),
        })
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, Infallible> {
        Ok(WrittenGeometry::deserialize(self))
    }
}

/// The members of a parsed object, as the rules read an object node.
#[must_use]
pub fn object_members(members: &Map<String, Value>) -> NodeMembers<'_, &Value> {
    NodeMembers::new(members.iter().map(|(name, value)| (Cow::Borrowed(name.as_str()), value)).collect())
}
