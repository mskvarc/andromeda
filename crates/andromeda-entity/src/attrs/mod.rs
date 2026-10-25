//! Attribute-level writes: clauses 5.6.2, 5.6.3, 5.6.4, 5.6.5 and 5.6.19.
//!
//! Every one of them is a read-modify-write: the entity is read, the mutation is
//! applied to a copy of its state in memory that shares every attribute it
//! leaves alone, and the whole state is written back with whatever the
//! mutation implies for the Temporal Evolution. None of
//! them holds a transaction across the read, so each states the `modifiedAt` its
//! result was computed from, see [`read_modify_write`] for why that matters and
//! what the alternative costs.
//!
//! [`read_modify_write`]: crate::read_modify_write

/// Appending attributes to an entity (clause 5.6.3).
pub mod append;
/// Deleting an attribute from an entity (clause 5.6.5).
pub mod delete;
/// Partially updating one attribute instance (clause 5.6.4).
pub mod partial_update;
/// Replacing one attribute wholesale (clause 5.6.19).
pub mod replace;
/// Updating the attributes an entity already has (clause 5.6.2).
pub mod update;

/// Durable-match recording tests for the attribute-level writes.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod durable_match_recording;
/// History-recording tests for the attribute-level writes.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod history_recording;
