//! Durable context-space keys.

use thiserror::Error;

/// Positive database-issued identity of one context space inside a tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextSpaceKey(i64);

impl ContextSpaceKey {
    /// Return the database representation.
    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.0
    }
}

// A derive cannot enforce the positive-key invariant required by database-issued identities.
impl TryFrom<i64> for ContextSpaceKey {
    type Error = ContextSpaceKeyError;

    fn try_from(value: i64) -> Result<ContextSpaceKey, ContextSpaceKeyError> {
        if value > 0 {
            Ok(ContextSpaceKey(value))
        } else {
            Err(ContextSpaceKeyError::NotPositive(value))
        }
    }
}

/// Validation failure for a persisted context-space key.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextSpaceKeyError {
    /// Database identities are strictly positive.
    #[error("context-space key must be positive, got {0}")]
    NotPositive(i64),
}

#[cfg(test)]
mod tests {
    use crate::context_space_key::{ContextSpaceKey, ContextSpaceKeyError};

    #[test]
    fn positive_database_key_is_accepted() {
        let key = ContextSpaceKey::try_from(7).expect("positive keys are valid");
        pretty_assertions::assert_eq!(key.as_i64(), 7);
    }

    #[test]
    fn non_positive_database_keys_are_rejected() {
        pretty_assertions::assert_eq!(ContextSpaceKey::try_from(0), Err(ContextSpaceKeyError::NotPositive(0)));
        pretty_assertions::assert_eq!(ContextSpaceKey::try_from(-1), Err(ContextSpaceKeyError::NotPositive(-1)));
    }
}
