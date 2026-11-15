//! In-memory evaluation of the `q` language (clause 4.9): running a filter's
//! reading, the attribute axis a Query Term walks to its targets, the target
//! value each attribute family carries, the test of each target, and the steps
//! a term's hops take into linked entities.

pub mod attribute_axis;
pub mod compound_member;
pub mod cost_class;
pub mod family_target;
pub mod filter_run;
pub mod geometry_member;
pub mod hop_steps;
pub mod language_target;
pub mod prepared_filter;
pub mod required_attributes;
pub mod target_value;
pub mod term_memo;
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

/// The normal form of a filter selects what the filter as lowered selects.
#[cfg(test)]
mod normal_form_tests;

/// A prepared filter and its required attributes hold for exactly the states
/// the filter holds for, over generated and corpus Entities.
#[cfg(test)]
mod prepared_filter_tests;
