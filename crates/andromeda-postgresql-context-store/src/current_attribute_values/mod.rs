//! The `current_attribute_values` projection of the entities an entity
//! mutation writes.
//!
//! The projection holds one row per attribute instance an entity currently
//! carries, at the clause 4.5.5 grain of entity, attribute name and
//! `datasetId`, with the instance's value in exactly one typed column. It is
//! what a pushed-down `q` filter, a geo-query and a distance ordering are
//! answered from, so it is written synchronously inside the transaction that
//! writes the entity itself: a projection failure rolls the mutation back
//! rather than leaving a query answering from a state no client ever wrote.
//!
//! Deletion is a deletion, never a marker. There is no tombstone row here and
//! no shape that could express one — an attribute instance that clause 5.6.6
//! or 5.6.13 removes has its projection row deleted, and an entity that goes
//! takes its rows with it through the one foreign key the table declares.

/// The column-major binds one projection batch is written from.
pub mod columns;
/// Removing projection rows an entity mutation no longer stands behind.
pub mod delete;
/// Deriving a projected value from a neutral observation value.
pub mod observed_value;
/// One projected attribute instance, and the grain it is written at.
pub mod row;
/// Writing an entity mutation's attribute instances into the projection.
pub mod upsert;
