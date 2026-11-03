//! A compacted document read as the compact model reads any compact tree.
//!
//! The compact model's reading ([`CompactNode`]) is the one a request gets, so
//! a compacted entity becomes exactly the [`Entity`] its JSON would read as. A
//! geometry the walk kept typed is handed over as the typed geometry it is;
//! a value the model holds as JSON is written from the tree and admitted.
//!
//! [`Entity`]: andromeda_ngsi_ld_model::entity::Entity

use crate::compacted::Compacted;
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use serde::de::DeserializeSeed;
use std::borrow::Cow;

impl<'a: 'n, 'n> CompactNode<'n> for &'n Compacted<'a> {
    type Unreadable = serde_json::Error;

    fn shape(self) -> NodeShape<'n, &'n Compacted<'a>> {
        match self {
            Compacted::Null => NodeShape::Null,
            Compacted::Boolean(flag) => NodeShape::Boolean(*flag),
            Compacted::Number(number) => NodeShape::Number(Cow::Borrowed(number.as_str())),
            Compacted::String(text) => NodeShape::String(Cow::Borrowed(text)),
            Compacted::Array(items) => NodeShape::Array(items.iter().collect()),
            Compacted::Object(members) => NodeShape::Object(NodeMembers::new(
                members.iter().map(|(name, value)| (Cow::Borrowed(name.as_str()), value)).collect(),
            )),
            Compacted::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, serde_json::Error> {
        serde_json::to_string(self).map(|text| JsonLiteral::admit(&text))
    }

    fn decoded_by<T, S: for<'de> DeserializeSeed<'de, Value = T>>(self, seed: S) -> Result<Result<T, serde_json::Error>, serde_json::Error> {
        serde_json::to_string(self).map(|text| seed.deserialize(&mut serde_json::Deserializer::from_str(&text)))
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, serde_json::Error> {
        serde_json::to_string(self).map(|text| serde_json::from_str(&text))
    }
}
