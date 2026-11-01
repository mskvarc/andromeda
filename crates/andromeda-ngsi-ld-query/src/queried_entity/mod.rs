//! What the query languages read of an entity.
//!
//! The `q` language (clause 4.9), the geo-query (clause 4.10) and the scope
//! query (clause 4.19) are evaluated in memory against entities this crate does
//! not define. Each capability an evaluator needs is a trait here, implemented
//! by whatever holds the entity, so every evaluator is written once against the
//! traits and the crate depends on no entity model.
//!
//! [`attribute_reading`] is what `q` reads: attribute instances by name, the
//! values of their family member, their sub-attributes and the instance members
//! listed in [`instance_metadata`]. [`linked_entities`] is where a `q` hop reads
//! the entities a Relationship links to. [`located_entity`] is what a geo-query
//! reads and [`scoped_entity`] what a scope query reads.

pub mod attribute_reading;
pub mod instance_metadata;
pub mod linked_entities;
pub mod located_entity;
pub mod scoped_entity;
pub mod value_item;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
pub(crate) mod expanded_fixture;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
pub(crate) mod fixture_literal;
