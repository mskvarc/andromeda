//! Entity-query request components.

pub mod admitted;
pub mod body_conversion;
pub mod operation;
pub mod parameters;
pub mod purge;

mod validation;

#[cfg(test)]
mod admission_tests;
