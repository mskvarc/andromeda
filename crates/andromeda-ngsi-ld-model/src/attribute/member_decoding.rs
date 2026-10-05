//! Decoding one attribute member, or one attribute instance, from any serde
//! deserializer.
//!
//! The member is written as the JSON text the deserializer delivers, admitted,
//! and read in place by the compact model's one reading of a node
//! ([`MemberReading::from_node`]), so a member decoded this way is exactly what
//! the same member of a request or a compacted entity reads as. A member read
//! apart from the entity it belongs to has no attribute name, so it is read as
//! an attribute that is not `ngsildproof` (clause 4.5.2.2).

use crate::{
    attribute::{
        instance::{Attribute, AttributeValue},
        member_reading::MemberReading,
    },
    node_refusal::NodeReadError,
};
use andromeda_json_literal::JsonLiteral;
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use std::convert::Infallible;

/// The attribute name a member read apart from its entity is read under.
const UNNAMED: &str = "";

// Hand-written: the member is read by the compact model's reading of a node,
// which no derive expresses.
impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AttributeValue, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        AttributeValue::from_node(UNNAMED, literal.piece()).map_err(refusal)
    }
}

// Hand-written for the same reason as the member's decoding.
impl<'de> Deserialize<'de> for Attribute {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Attribute, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        Attribute::from_node(UNNAMED, literal.piece()).map_err(refusal)
    }
}

/// The deserializer's error for a member the reading refuses.
pub(crate) fn refusal<E: DeserializeError>(error: NodeReadError<Infallible>) -> E {
    match error {
        NodeReadError::Refused(refused) => E::custom(refused),
        NodeReadError::Unreadable(never) => match never {},
    }
}
