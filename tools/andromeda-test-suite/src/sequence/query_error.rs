//! Why a query yields no value to check.

use crate::sequence::{queried::Queried, query::Filter, variables::TemplateError};
use serde_json_path::ParseError;

/// Why a query yields no value to check.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The body is not UTF-8 text.
    #[error("the body is not UTF-8 text")]
    NotText,
    /// The body is not JSON.
    #[error("the body is not JSON: {0}")]
    BodyNotJson(#[source] serde_json::Error),
    /// The string a JSON path filter reads is not JSON.
    #[error("{value} is not JSON: {source}")]
    ValueNotJson {
        /// The string read.
        value: Box<Queried>,
        /// Why it does not read as JSON.
        #[source]
        source: serde_json::Error,
    },
    /// The JSON path expression is not valid.
    #[error("the filled expression is not a JSONPath expression: {0}")]
    Expression(#[source] ParseError),
    /// The regular expression is not valid.
    #[error("the filled pattern is not a regular expression: {0}")]
    Pattern(#[source] regex::Error),
    /// A filter cannot read the value it is given.
    #[error("{filter} cannot read {value}")]
    Unreadable {
        /// The filter.
        filter: Box<Filter>,
        /// The value.
        value: Box<Queried>,
    },
    /// An `nth` filter names a position past the end of the list it reads.
    #[error("{filter} cannot read a list of {length}")]
    PastTheEnd {
        /// The filter.
        filter: Box<Filter>,
        /// The length of the list read.
        length: usize,
    },
    /// A template of the query cannot be filled.
    #[error(transparent)]
    Template(#[from] TemplateError),
}
