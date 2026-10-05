//! Whether a mutation reached the registration it named.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;

/// Outcome of a mutation against one registration identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationMutation {
    /// One registration was changed, by a write that drew this revision.
    Applied(CsourceChangeRevision),
    /// No registration matched the identifier.
    Missing,
}

impl RegistrationMutation {
    /// Require a mutation to have changed the requested registration,
    /// returning the revision the change drew.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when the mutation matched no
    /// registration carrying `identifier`.
    pub fn require_applied(self, identifier: &Identifier) -> Result<CsourceChangeRevision, RegistrationError> {
        match self {
            RegistrationMutation::Applied(revision) => Ok(revision),
            RegistrationMutation::Missing => Err(RegistrationError::not_found(identifier)),
        }
    }
}
