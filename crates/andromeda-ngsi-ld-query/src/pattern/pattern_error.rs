//! Why a regular expression of an `idPattern` or of a `q` pattern operand is
//! refused.

use crate::pattern::non_posix_construct::NonPosixConstruct;
use regex_syntax::hir;
use thiserror::Error;

/// The rule a regular expression broke, against IEEE 1003.2 or against the
/// dialect the broker admits.
#[derive(Debug, Error)]
pub enum PatternError {
    /// The `regex` crate refused the pattern.
    #[error("invalid regular expression: {0}")]
    Regex(#[from] regex::Error),
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
    /// A pattern the `regex` crate compiled could not be translated into the
    /// intermediate representation its store restatement is rendered from.
    #[error("invalid regular expression: {0}")]
    Translation(#[from] hir::Error),
}
