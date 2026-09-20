//! Failures reported by Entity Map policy and storage implementations.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use thiserror::Error;

/// A semantic Entity Map failure or an implementation-neutral storage failure.
#[derive(Debug, Error)]
pub enum EntityMapError {
    /// The requested Entity Map is absent or has expired.
    #[error("entity map {identifier} not found")]
    NotFound {
        /// The requested Entity Map identifier.
        identifier: Uri,
    },
    /// A follow-up request under a live map changed a filter parameter of
    /// the request that created it, which clause 5.5.14 forbids.
    #[error("the request under entity map {identifier} does not use the parameters of the request that created it")]
    RequestChanged {
        /// The Entity Map the request referenced.
        identifier: Uri,
    },
    /// A storage implementation could not complete an operation.
    #[error("entity map storage failed: {detail}")]
    Persistence {
        /// Human-readable backend-neutral failure detail.
        detail: String,
    },
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
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
        pretty_assertions::assert_eq!(error.to_string(), "entity map urn:ngsi-ld:EntityMap:missing not found");
        Ok(())
    }
}
