//! In-memory evaluation of the `q` language (clause 4.9): the operators
//! clause 4.9 applies, the attribute axis a Query Term walks to its targets,
//! the target value each attribute family carries, and the steps a term's
//! hops take into linked entities.

pub mod attribute_axis;
pub mod axis_position;
pub mod compound_member;
pub mod eval;
pub mod family_target;
pub mod hop_steps;
pub mod language_target;
pub mod target_value;

/// A filter read back from storage evaluates with the members its client wrote.
#[cfg(test)]
mod persisted_filter_tests;

/// Every corpus filter selects the Entities clause 4.9 names, in memory.
#[cfg(test)]
mod corpus_tests;
