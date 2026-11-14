//! In-memory evaluation of the `q` language (clause 4.9): running a filter's
//! reading, the attribute axis a Query Term walks to its targets, the target
//! value each attribute family carries, the test of each target, and the steps
//! a term's hops take into linked entities.

pub mod attribute_axis;
pub mod compound_member;
pub mod family_target;
pub mod filter_run;
pub mod hop_steps;
pub mod language_target;
pub mod target_value;
pub mod verdict_run;

/// Every operator, path shape and attribute family, run in memory.
#[cfg(test)]
mod filter_run_tests;

/// A filter read back from storage evaluates with the members its client wrote.
#[cfg(test)]
mod persisted_filter_tests;

/// Every corpus filter selects the Entities clause 4.9 names, in memory.
#[cfg(test)]
mod corpus_tests;
