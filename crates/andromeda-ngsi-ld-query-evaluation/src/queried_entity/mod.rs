//! What the query languages read of an entity, and of a resource that is not
//! one.
//!
//! The `q` language (clause 4.9), the geo-query (clause 4.10) and the scope
//! query (clause 4.19) are evaluated in memory against entities this crate does
//! not define. Each capability an evaluator needs is a trait here, implemented
//! by whatever holds the entity, so every evaluator is written once against the
//! traits and the crate depends on no entity model.
//!
//! [`attribute_reading`] is what `q` reads: attribute instances by name, the
//! values of their family member, their sub-attributes and the members an
//! instance holds as data of its own, and [`queried_geometry`] the `GeoJSON`
//! object a `GeoProperty`'s geometry is. [`linked_entities`] is where a `q` hop
//! reads the entities a Relationship links to. [`located_entity`] is what a
//! geo-query reads and [`scoped_entity`] what a scope query reads.
//! [`resource_members`] is what `q` reads of a resource that is no entity.

pub mod attribute_reading;
pub mod linked_entities;
pub mod located_entity;
pub mod queried_geometry;
pub mod raw_json;
pub mod resource_members;
pub mod scoped_entity;
pub mod value_item;

#[cfg(test)]
pub(crate) mod corpus_fixture;
#[cfg(test)]
pub(crate) mod expanded_fixture;
#[cfg(test)]
pub(crate) mod fixture_geometry;
#[cfg(test)]
pub(crate) mod fixture_literal;
#[cfg(test)]
pub(crate) mod placed_fixture;
