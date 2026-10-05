//! Entities written straight from their typed form into a response body.
//!
//! The renderings of clause 4.5 -- normalized, concise, simplified and the
//! `GeoJSON` envelope -- are written member by member from the typed
//! [`andromeda_ngsi_ld_model::entity::Entity`], with the clause 4.21
//! structural rule, the clause 4.15 language negotiation, the clause 4.5.23
//! inline join and the presentation-boundary URI decoding applied on the way,
//! so a response never holds a second, untyped copy of the entity and a
//! `GeoProperty` geometry goes to the wire from its positions.

pub(crate) mod family_value;
pub(crate) mod feature_body;
pub(crate) mod feature_geometry;
pub(crate) mod linked_entities;
pub(crate) mod member_walk;
pub(crate) mod notified_member;
pub(crate) mod object_list_body;
pub(crate) mod reified_instance;
pub(crate) mod reified_member;
pub(crate) mod simplified_member;
pub(crate) mod typed_entity;
pub(crate) mod value_body;
