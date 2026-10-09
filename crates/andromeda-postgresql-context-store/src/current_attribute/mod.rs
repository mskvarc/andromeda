//! The `current_attributes` rows an Entity's attributes are stored as.
//!
//! One row holds one top-level attribute instance at the clause 4.5.5 grain of
//! Entity, attribute name and `datasetId`, its value in exactly one typed
//! column, and its sub-attributes (clause 4.5.2) as one array of
//! `sub_attribute` elements. The rows are the only copy of an Entity's
//! attributes: flattening a committed state and assembling its rows give back
//! the same state.

/// Assembling an Entity's attributes from its rows.
pub mod assemble;
/// Flattening an Entity's attributes into rows.
pub mod flatten;
/// The dictionary keys rows hold names as.
pub mod name_keys;
#[cfg(test)]
mod round_trip_tests;
/// One row.
pub mod row;
/// The family value an instance's stored columns hold.
mod stored_family;
/// A JSON piece held as its codec wrote it.
pub mod stored_json;
/// The value of one instance, by the column that holds it.
pub mod stored_value;
/// One sub-attribute element and the composite it travels as.
pub mod sub_attribute;
