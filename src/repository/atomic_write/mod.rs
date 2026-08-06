//! The PostgreSQL implementation of the atomic-write port.

/// The anchor store and the transaction it opens.
pub mod anchor;
/// Building PostgreSQL bind columns from the NGSI-LD side of a write.
mod columns;
/// Sorting writes into fused statements and reading outcomes back out.
mod entity_group;
/// Appending the history part of a set of writes.
mod history;
