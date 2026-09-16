//! The broker-internal record of the request that created an `EntityMap`.
//!
//! Clause 5.5.14: "Subsequent requests referencing an `EntityMap` shall use the
//! same parameters as in the original request that created the `EntityMap`,
//! except for the specification of Entity identifiers or parameters related
//! to pagination, or, in the case of temporal requests, the temporal query."
//! Enforcing that needs the creating request remembered beside the map, as
//! an opaque digest of its filter parameters. The fingerprint is never a
//! member of the clause 5.2.39 representation.

use std::str::FromStr;
use strum::{Display, EnumString};
use thiserror::Error;

/// The operation that created an `EntityMap` (clause 5.5.14 lists the four).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum MapRequestKind {
    /// Query Entities (clause 5.7.2) or Create `EntityMap` for Query Entities (clause 5.14.4).
    QueryEntities,
    /// Retrieve Entity (clause 5.7.1).
    RetrieveEntity,
    /// Query Temporal Evolution of Entities (clause 5.7.4) or its map creation (clause 5.14.5).
    QueryTemporal,
    /// Retrieve Temporal Evolution of an Entity (clause 5.7.3).
    RetrieveTemporal,
}

/// How a follow-up request's fingerprint relates to the one that created the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestComparison {
    /// The same operation with the same filter parameters.
    Same,
    /// The same operation with different filter parameters, which clause
    /// 5.5.14 forbids under the map.
    Changed,
    /// A different operation, whose parameters are not comparable: a
    /// retrieval under a map a query created carries no query filters.
    DifferentOperation,
}

/// The digest of the filter parameters of the request that created a map,
/// tagged with the operation it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{}:{}", kind, hex::encode(digest))]
pub struct RequestFingerprint {
    /// The creating operation.
    kind: MapRequestKind,
    /// The SHA-256 over the canonical filter parameters.
    digest: [u8; 32],
}

impl RequestFingerprint {
    /// A fingerprint of `kind` over an already computed digest.
    #[must_use]
    pub const fn new(kind: MapRequestKind, digest: [u8; 32]) -> RequestFingerprint {
        RequestFingerprint { kind, digest }
    }

    /// The operation that created the map.
    #[must_use]
    pub const fn kind(&self) -> MapRequestKind {
        self.kind
    }

    /// Compare a follow-up request's fingerprint with the creating one.
    #[must_use]
    pub fn compare(&self, follow_up: &RequestFingerprint) -> RequestComparison {
        if self.kind != follow_up.kind {
            RequestComparison::DifferentOperation
        } else if self.digest == follow_up.digest {
            RequestComparison::Same
        } else {
            RequestComparison::Changed
        }
    }
}

/// Why stored text is not a [`RequestFingerprint`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("entity map request fingerprint '{0}' is not <operation>:<64 lowercase hex digits>")]
pub struct RequestFingerprintError(String);

// The stored form carries two fields behind one separator, which no derive
// can split and validate.
impl FromStr for RequestFingerprint {
    type Err = RequestFingerprintError;

    fn from_str(value: &str) -> Result<RequestFingerprint, RequestFingerprintError> {
        let (kind, digest) = value.split_once(':').ok_or_else(|| RequestFingerprintError(value.to_owned()))?;
        let kind = kind.parse::<MapRequestKind>().map_err(|_| RequestFingerprintError(value.to_owned()))?;
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(digest, &mut bytes).map_err(|_| RequestFingerprintError(value.to_owned()))?;
        if hex::encode(bytes) != digest {
            return Err(RequestFingerprintError(value.to_owned()));
        }
        Ok(RequestFingerprint { kind, digest: bytes })
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map_fingerprint::{MapRequestKind, RequestComparison, RequestFingerprint, RequestFingerprintError};

    #[test]
    fn the_stored_form_names_the_operation_and_round_trips() -> Result<(), RequestFingerprintError> {
        let fingerprint = RequestFingerprint::new(MapRequestKind::QueryTemporal, [0xab; 32]);
        let stored = fingerprint.to_string();
        assert!(stored.starts_with("query-temporal:abab"), "{stored}");
        pretty_assertions::assert_eq!(stored.parse::<RequestFingerprint>()?, fingerprint);
        Ok(())
    }

    #[test]
    fn malformed_stored_text_is_refused() {
        assert!("query-entities".parse::<RequestFingerprint>().is_err());
        assert!("purge:00".parse::<RequestFingerprint>().is_err());
        assert!(format!("retrieve-entity:{}", "0".repeat(63)).parse::<RequestFingerprint>().is_err());
        assert!(format!("retrieve-entity:{}", "A".repeat(64)).parse::<RequestFingerprint>().is_err());
    }

    #[test]
    fn comparison_distinguishes_same_changed_and_other_operations() {
        let created = RequestFingerprint::new(MapRequestKind::QueryEntities, [1; 32]);
        pretty_assertions::assert_eq!(
            created.compare(&RequestFingerprint::new(MapRequestKind::QueryEntities, [1; 32])),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.compare(&RequestFingerprint::new(MapRequestKind::QueryEntities, [2; 32])),
            RequestComparison::Changed
        );
        pretty_assertions::assert_eq!(
            created.compare(&RequestFingerprint::new(MapRequestKind::RetrieveEntity, [2; 32])),
            RequestComparison::DifferentOperation
        );
    }
}
