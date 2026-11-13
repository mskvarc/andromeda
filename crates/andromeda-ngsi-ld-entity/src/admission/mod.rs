//! The clause 5.5.4 rules a written document is held to before a write is
//! built from it, and the attribute nesting the broker holds it to so that
//! what it stores reads back.

pub mod attribute_nesting_admission;
pub mod datatype_admission;
pub mod document_admission;
pub mod geo_member_admission;
pub mod instance_completeness;
pub mod null_sentinel_admission;
pub mod written_families;

#[cfg(test)]
mod document_admission_tests;
#[cfg(test)]
mod geo_member_admission_tests;
