//! The clause 5.5.4 rules a written document is held to before a write is
//! built from it, and the attribute nesting the broker holds it to so that
//! what it stores reads back.

pub mod attribute_nesting_admission;
pub mod datatype_admission;
pub mod document_admission;
pub mod instance_completeness;
pub mod null_sentinel_admission;
pub mod stored_text_admission;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod document_admission_tests;
