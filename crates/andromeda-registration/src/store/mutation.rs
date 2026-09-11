//! Whether a mutation reached the registration it named.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::uri::Uri;

/// Outcome of a mutation against one registration identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationMutation {
    /// One registration was changed.
    Applied,
    /// No registration matched the identifier.
    Missing,
}

impl RegistrationMutation {
    /// Require a mutation to have changed the requested registration.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when the mutation matched no
    /// registration carrying `identifier`.
    pub fn require_applied(self, identifier: &Uri) -> Result<(), RegistrationError> {
        match self {
            RegistrationMutation::Applied => Ok(()),
            RegistrationMutation::Missing => Err(RegistrationError::not_found(identifier)),
        }
    }
}
