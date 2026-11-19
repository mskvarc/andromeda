//! Why a regular expression of an `idPattern` or of a `q` pattern operand is
//! refused.

use crate::pattern::non_posix_construct::NonPosixConstruct;
use regex_syntax::{ast, hir};
use thiserror::Error;

/// The rule a regular expression broke, against IEEE 1003.2 or against the
/// dialect the broker admits.
///
/// Every refusal is one line naming the rule: the parser's own errors draw
/// the pattern with a marker under the fault, over several lines, so only
/// their kind is kept.
#[derive(Debug, Error)]
pub enum PatternError {
    /// The pattern is no regular expression at all.
    #[error("invalid regular expression: {0}")]
    Syntax(ast::ErrorKind),
    /// A repetition operator immediately followed another repetition operator, which PostgreSQL's POSIX engine rejects.
    #[error("invalid POSIX regular expression: a repetition operator cannot follow another repetition operator")]
    RepeatedQuantifier,
    /// The pattern used a construct outside the dialect the broker admits.
    ///
    /// Clause 4.9 mandates IEEE 1003.2. The construct named here is either an extension the broker does not support, or one of the few IEEE 1003.2 constructs the `regex` crate reads differently from POSIX.2; admitting it would make the filter mean something other than what IEEE 1003.2 or the client intended.
    #[error("invalid POSIX regular expression: {construct} is not supported")]
    NonPosixConstruct {
        /// The unsupported construct.
        construct: NonPosixConstruct,
    },
    /// A parsed pattern names what no pattern can match, such as an unknown
    /// Unicode class.
    #[error("invalid regular expression: {0}")]
    Translation(hir::ErrorKind),
    /// A valid pattern compiles past the size the in-memory engine admits.
    #[error("invalid regular expression: the pattern compiles past the size the broker admits")]
    TooLarge(#[source] regex::Error),
}
