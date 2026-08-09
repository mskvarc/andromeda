//! Failures reported by Entity Map policy and storage implementations.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use thiserror::Error;

/// A semantic Entity Map failure or an implementation-neutral storage failure.
#[derive(Debug, Error)]
pub enum EntityMapError {
    /// The requested Entity Map is absent or has expired.
    #[error("entity map not found: {identifier}")]
    NotFound {
        /// The requested Entity Map identifier.
        identifier: Uri,
    },
    /// An entity expected in a map is not one of its members.
    #[error("entity {entity_identifier} is not present in entity map {map_identifier}")]
    EntityNotInMap {
        /// The Entity Map whose membership was checked.
        map_identifier: Uri,
        /// The entity identifier that was not present.
        entity_identifier: Uri,
    },
    /// A storage implementation could not complete an operation.
    #[error("entity map storage failed: {detail}")]
    Persistence {
        /// Human-readable backend-neutral failure detail.
        detail: String,
    },
    /// Policy generated a value that violated a domain invariant.
    #[error("entity map invariant failed: {detail}")]
    Invariant {
        /// Human-readable invariant failure detail.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::EntityMapError;
    use andromeda_ngsi_ld_lexicon::uri::Uri;

    #[test]
    fn missing_map_error_names_its_identifier() -> Result<(), EntityMapError> {
        let identifier = "urn:ngsi-ld:EntityMap:missing"
            .parse::<Uri>()
            .map_err(|error| EntityMapError::Invariant { detail: error.to_string() })?;
        let error = EntityMapError::NotFound { identifier };
        pretty_assertions::assert_eq!(error.to_string(), "entity map not found: urn:ngsi-ld:EntityMap:missing");
        Ok(())
    }
}
