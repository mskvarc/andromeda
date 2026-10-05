//! Decoding an [`Entity`] and an [`EntityPatch`] from any serde deserializer.
//!
//! The entity is written as the JSON text the deserializer delivers, admitted,
//! and read in place by the compact model's one reading of a node
//! ([`Entity::from_node`]), so an entity decoded this way is exactly what the
//! same entity of a request or a compacted document reads as. Nothing builds a
//! JSON tree on the way.

use crate::{
    attribute::{member_decoding::refusal, member_reading::MemberReading},
    entity::{Entity, EntityPatch},
};
use andromeda_json_literal::JsonLiteral;
use serde::{Deserialize, Deserializer};

// Hand-written: the entity is read by the compact model's reading of a node,
// which no derive expresses.
impl<'de, M: MemberReading> Deserialize<'de> for Entity<M> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Entity<M>, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        Entity::<M>::from_node(literal.piece()).map_err(refusal)
    }
}

// Hand-written for the same reason as the entity's decoding.
impl<'de, M: MemberReading> Deserialize<'de> for EntityPatch<M> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<EntityPatch<M>, D::Error> {
        let literal = JsonLiteral::deserialize(deserializer)?;
        EntityPatch::<M>::from_node(literal.piece()).map_err(refusal)
    }
}
