//! The clause 5.5.4 rules a written document is held to before a write is
//! built from it, and the attribute nesting the broker holds it to so that
//! what it stores reads back.

pub mod attribute_nesting_admission;
pub mod document_admission;
pub mod instance_completeness;
pub mod null_sentinel_admission;
pub mod time_value_admission;

#[cfg(test)]
mod document_admission_tests;
