//! A malformed `aggrGroupBy` value.

use crate::{query_error::QueryError, temporal::grouping::bucket_term::BucketTerm};
use thiserror::Error;

/// A malformed `aggrGroupBy` value.
#[derive(Debug, Error)]
pub enum GroupingError {
    /// The whole parameter was empty or blank.
    #[error("aggrGroupBy must name at least one grouping term")]
    Empty,
    /// A term between commas was empty (a leading, trailing or doubled comma).
    #[error("aggrGroupBy has an empty grouping term")]
    EmptyTerm,
    /// A grouping key was not a well-formed clause 4.9 `ValuePath`.
    #[error("invalid aggrGroupBy key: {0}")]
    Key(#[source] QueryError),
    /// A bucket modifier used an unknown token.
    #[error("invalid aggrGroupBy bucket: {0}")]
    Bucket(#[source] strum::ParseError),
    /// A bucket modifier was attached to a key that is not a temporal property.
    ///
    /// A bucket truncates or folds a timestamp, so it is meaningful only on one
    /// of the clause 4.8 temporal properties; on any other key it names no
    /// value the bucket could apply to.
    #[error("aggrGroupBy bucket '{0}' is on a key that is not a temporal property: a bucket applies only to a temporal property")]
    BucketOnNonTemporalKey(BucketTerm),
}
