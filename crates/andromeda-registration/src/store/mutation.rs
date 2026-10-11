//! Whether a mutation reached the registration it named.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Outcome of a mutation against one registration identifier, carrying what
/// the applied mutation reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationMutation<T> {
    /// One registration was changed, and the store reports this about it.
    Applied(T),
    /// No registration matched the identifier.
    Missing,
}

impl<T> RegistrationMutation<T> {
    /// Require a mutation to have changed the requested registration,
    /// returning what the store reported about the change.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when the mutation matched no
    /// registration carrying `identifier`.
    pub fn require_applied(self, identifier: &Identifier) -> Result<T, RegistrationError> {
        match self {
            RegistrationMutation::Applied(applied) => Ok(applied),
            RegistrationMutation::Missing => Err(RegistrationError::not_found(identifier)),
        }
    }
}
