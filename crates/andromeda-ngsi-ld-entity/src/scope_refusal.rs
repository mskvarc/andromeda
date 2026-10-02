//! Why the entity `scope` member is not a value clause 4.18 admits.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePathError;
use thiserror::Error;

/// Why the `scope` member is not a value clause 4.18 admits.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ScopeRefusal {
    /// One node of the member carries no string value.
    #[error("a scope value is not a string")]
    NotAString,
    /// One value is not a scope path.
    #[error("a scope value is not a scope path: {source}")]
    NotAScopePath {
        /// Why it is not a scope path.
        source: ScopePathError,
    },
    /// The NGSI-LD Null appears beside a scope path; clause 4.18 admits it only
    /// "in case of deleted scopes".
    #[error("the NGSI-LD Null names a deleted scope, so it cannot appear beside a scope path")]
    NullBesidePath,
    /// The NGSI-LD Null appears more than once; clause 4.18: "There shall at
    /// most be one instance of the scope property per Entity".
    #[error("the NGSI-LD Null appears more than once in the scope member, which states at most one deleted scope")]
    RepeatedNull,
    /// The member lists no path at all.
    #[error("the scope member lists no scope path")]
    Empty,
}
