//! One Entity's state as the `entity_state_record` composite, the form a queue
//! stores a state in, and the assembly of a state from its stored parts, which
//! queue reads and Entity reads share.
//!
//! A record holds what the Entity's `entities` and `current_attributes` rows
//! hold: its type keys, its Scopes as written, its `expiresAt`, and one
//! `attribute_instance_record` per row, flattened and assembled by the same
//! functions as the rows. A state therefore crosses a queue as typed fields,
//! never as JSON text.

/// One top-level instance as an `attribute_instance_record`.
pub mod instance_record;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod round_trip_tests;
/// One state as an `entity_state_record`.
pub mod state_record;
/// A state's stored parts, and their assembly once the names are looked up.
pub mod stored_state;
