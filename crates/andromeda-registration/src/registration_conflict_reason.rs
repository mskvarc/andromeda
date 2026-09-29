//! The concrete reasons a registration collides with existing context data.
//!
//! [`RegistrationError::Conflict`](crate::error::RegistrationError::Conflict)
//! carries one of these. Each names the colliding footprint with typed
//! identifiers rather than an ad-hoc string; the clauses (5.9.2.4, 5.9.3.4,
//! 4.3.6.3) stay in doc comments.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// Why a proxied registration cannot be admitted against existing data.
#[derive(Debug, Error)]
pub enum RegistrationConflictReason {
    /// Another registration already covers the same entity and attributes.
    #[error("registration {registration} already covers entity {entity} for the registered attributes")]
    RegistrationCoversEntity {
        /// The registration whose footprint overlaps.
        registration: Identifier,
        /// The entity both registrations cover.
        ///
        /// Boxed to keep this variant, which already carries the registration
        /// `Identifier`, from being two `Identifier`s wide and enlarging every `Result` in the
        /// crate (and in federation, which wraps this error).
        entity: Box<Identifier>,
    },
    /// A local entity already holds an attribute an exclusive registration claims.
    #[error("entity {entity} already holds one of the attributes the exclusive registration covers")]
    EntityHoldsCoveredAttribute {
        /// The entity holding a covered attribute.
        entity: Identifier,
    },
    /// A local entity already matches a redirect registration's footprint.
    #[error("a local entity already matches the redirect registration's entity and attribute footprint")]
    LocalEntityMatchesRedirect,
}
