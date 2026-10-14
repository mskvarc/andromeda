//! The refusals a structured query-string parameter reports while it is read.

use std::num::ParseIntError;
use thiserror::Error;

/// A query-string parameter whose value cannot be read as the parameter requires.
#[derive(Debug, Error)]
pub enum QueryParameterError {
    /// `idPattern` was empty or blank.
    #[error("invalid idPattern: empty value")]
    IdPatternEmpty,
    /// A mutation endpoint's `datasetId` carried a comma, so it named a list.
    #[error("datasetId must contain one URI or @none selector")]
    DatasetIdNotSingleton,
    /// A mutation endpoint's `datasetId` was empty or blank.
    #[error("datasetId must not be empty")]
    DatasetIdEmpty,
    /// `joinLevel` was an integer below 1 or beyond `u32`.
    #[error("joinLevel {level} is not a positive integer")]
    JoinLevelOutOfRange {
        /// The integer the parameter carried.
        level: i64,
    },
    /// `joinLevel` was an integer too large for any machine integer.
    #[error("joinLevel is not a positive integer: {0}")]
    JoinLevelOverflow(#[source] ParseIntError),
    /// A duration parameter was not an ISO 8601 duration.
    #[error("not an ISO 8601 duration at position {position}")]
    DurationInvalid {
        /// Zero-based byte offset at which the duration grammar failed.
        position: usize,
    },
}

#[cfg(test)]
mod tests {
    use crate::query::parameter_error::QueryParameterError;

    #[test]
    fn an_out_of_range_join_level_names_the_integer_it_carried() {
        pretty_assertions::assert_eq!(
            QueryParameterError::JoinLevelOutOfRange { level: -1 }.to_string(),
            "joinLevel -1 is not a positive integer"
        );
    }

    #[test]
    fn a_malformed_duration_names_where_the_grammar_failed() {
        pretty_assertions::assert_eq!(
            QueryParameterError::DurationInvalid { position: 2 }.to_string(),
            "not an ISO 8601 duration at position 2"
        );
    }
}
