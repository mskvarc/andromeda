//! A `q` filter with every clause 4.9 decision taken.

use crate::filter::reading::term_reading::TermReading;

/// The boolean structure of a lowered `q` filter over its term readings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterReading {
    /// Every child holds (`;`).
    All(Vec<FilterReading>),
    /// Some child holds (`|`).
    Any(Vec<FilterReading>),
    /// One Query Term.
    Term(Box<TermReading>),
}
