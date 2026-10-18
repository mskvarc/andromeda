//! Context Source Registration discovery queries (clause 5.10.2).
//!
//! A registration describes what a context source *may* hold, so its filters
//! are permissive where an entity query's are not: a registration that declares
//! no location or no temporal interval still matches a geo or temporal filter.

pub mod compiled;
pub mod expanded_entity_specification;
