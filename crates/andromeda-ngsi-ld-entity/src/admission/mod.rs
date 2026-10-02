//! The clause 5.5.4 rules a written document is held to before a write is
//! built from it.

pub mod document_admission;
pub mod null_sentinel_admission;

#[cfg(test)]
mod document_admission_tests;
