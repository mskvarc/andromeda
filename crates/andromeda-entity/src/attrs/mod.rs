//! Attribute-level writes: clauses 5.6.2, 5.6.3, 5.6.4, 5.6.5 and 5.6.19.
//!
//! Every one of them is a read-modify-write: the entity is read, the mutation is
//! applied to a copy of its payload in memory, and the whole payload is written
//! back with whatever the mutation implies for the Temporal Evolution. None of
//! them holds a transaction across the read, so each states the `modifiedAt` its
//! result was computed from — see [`read_modify_write`] for why that matters and
//! what the alternative costs.
//!
//! [`read_modify_write`]: crate::read_modify_write

/// Appending attributes to an entity (clause 5.6.3).
pub mod append;
/// The shape of an attribute-level write and what it produces.
pub mod attribute_write;
/// Deleting an attribute from an entity (clause 5.6.5).
pub mod delete;
/// Mapping an expanded-entity mutation failure onto a ProblemDetails type.
pub mod mutation_error;
/// Partially updating one attribute instance (clause 5.6.4).
pub mod partial_update;
/// Replacing one attribute wholesale (clause 5.6.19).
pub mod replace;
/// Updating the attributes an entity already has (clause 5.6.2).
pub mod update;

#[cfg(test)]
mod durable_match_recording;
/// History-recording tests for the attribute-level writes.
#[cfg(test)]
mod history_recording;
