//! The broker-internal record of the request that created an `EntityMap`.
//!
//! Clause 5.5.14: "Subsequent requests referencing an `EntityMap` shall use the
//! same parameters as in the original request that created the `EntityMap`,
//! except for the specification of Entity identifiers or parameters related
//! to pagination, or, in the case of temporal requests, the temporal query."
//! Enforcing that needs the creating request remembered beside the map, as two
//! opaque digests: one of what its compiled query selects, and one of every
//! other input the clause holds a follow-up to, its `@context` among them. A
//! purge reuses a map without being one of the operations the clause names, so
//! it is held to the first digest alone. The fingerprint is never a member of
//! the clause 5.2.39 representation.

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
    /// The same operation with the same parameters.
    Same,
    /// The same operation with different parameters, which clause 5.5.14
    /// forbids under the map.
    Changed,
    /// A different operation, whose parameters are not comparable: a
    /// retrieval under a map a query created carries no query filters.
    DifferentOperation,
}

/// The digest of what a request's compiled query selects, tagged with the
/// operation it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryFingerprint {
    /// The operation the query belongs to.
    kind: MapRequestKind,
    /// The SHA-256 over the canonical identity of the compiled query.
    digest: [u8; 32],
}

impl QueryFingerprint {
    /// A query fingerprint of `kind` over an already computed digest.
    #[must_use]
    pub const fn new(kind: MapRequestKind, digest: [u8; 32]) -> QueryFingerprint {
        QueryFingerprint { kind, digest }
    }

    /// The operation the query belongs to.
    #[must_use]
    pub const fn kind(&self) -> MapRequestKind {
        self.kind
    }

    /// Compare another request's query with this one.
    #[must_use]
    pub fn compare(&self, other: &QueryFingerprint) -> RequestComparison {
        if self.kind != other.kind {
            RequestComparison::DifferentOperation
        } else if self.digest == other.digest {
            RequestComparison::Same
        } else {
            RequestComparison::Changed
        }
    }
}

/// The digests of the request that created a map: what its compiled query
/// selects, and every other input clause 5.5.14 holds a follow-up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{}:{}:{}", query.kind, hex::encode(query.digest), hex::encode(inputs))]
pub struct RequestFingerprint {
    /// What the compiled query selects, tagged with the creating operation.
    query: QueryFingerprint,
    /// The SHA-256 over the canonical record of the request's other inputs.
    inputs: [u8; 32],
}

impl RequestFingerprint {
    /// A fingerprint of the request whose query is `query` and whose other
    /// inputs digest to `inputs`.
    #[must_use]
    pub const fn new(query: QueryFingerprint, inputs: [u8; 32]) -> RequestFingerprint {
        RequestFingerprint { query, inputs }
    }

    /// The operation that created the map.
    #[must_use]
    pub const fn kind(&self) -> MapRequestKind {
        self.query.kind
    }

    /// What the creating request's compiled query selects.
    #[must_use]
    pub const fn query(&self) -> &QueryFingerprint {
        &self.query
    }

    /// Compare a follow-up request's fingerprint with the creating one.
    #[must_use]
    pub fn compare(&self, follow_up: &RequestFingerprint) -> RequestComparison {
        match self.query.compare(&follow_up.query) {
            RequestComparison::Same if self.inputs != follow_up.inputs => RequestComparison::Changed,
            comparison @ (RequestComparison::Same | RequestComparison::Changed | RequestComparison::DifferentOperation) => comparison,
        }
    }
}

/// Why stored text is not a [`RequestFingerprint`].
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RequestFingerprintError {
    /// The text is not three fields joined by `:`.
    #[error("entity map request fingerprint is not <operation>:<64 lowercase hex digits>:<64 lowercase hex digits>")]
    MissingSeparator,
    /// The text before the first `:` names no known operation.
    #[error("entity map request fingerprint names no known operation: {0}")]
    UnknownOperation(#[source] strum::ParseError),
    /// A digest is not 64 hex digits.
    #[error("entity map request fingerprint digest is not 64 hex digits: {0}")]
    Digest(#[source] hex::FromHexError),
    /// A digest is hex, but not in the lowercase spelling the broker writes.
    #[error("entity map request fingerprint digest is not lowercase hex")]
    NonCanonicalDigest,
}

/// The 32 bytes `text` spells in the lowercase hex the broker writes.
fn canonical_digest(text: &str) -> Result<[u8; 32], RequestFingerprintError> {
    let mut bytes = [0_u8; 32];
    hex::decode_to_slice(text, &mut bytes).map_err(RequestFingerprintError::Digest)?;
    if hex::encode(bytes) != text {
        return Err(RequestFingerprintError::NonCanonicalDigest);
    }
    Ok(bytes)
}

// The stored form carries three fields behind two separators, which no derive
// can split and validate.
impl FromStr for RequestFingerprint {
    type Err = RequestFingerprintError;

    fn from_str(value: &str) -> Result<RequestFingerprint, RequestFingerprintError> {
        let (kind, digests) = value.split_once(':').ok_or(RequestFingerprintError::MissingSeparator)?;
        let (query, inputs) = digests.split_once(':').ok_or(RequestFingerprintError::MissingSeparator)?;
        let kind = kind.parse::<MapRequestKind>().map_err(RequestFingerprintError::UnknownOperation)?;
        Ok(RequestFingerprint {
            query: QueryFingerprint {
                kind,
                digest: canonical_digest(query)?,
            },
            inputs: canonical_digest(inputs)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map_fingerprint::{MapRequestKind, QueryFingerprint, RequestComparison, RequestFingerprint, RequestFingerprintError};

    fn fingerprint(kind: MapRequestKind, query: u8, inputs: u8) -> RequestFingerprint {
        RequestFingerprint::new(QueryFingerprint::new(kind, [query; 32]), [inputs; 32])
    }

    #[test]
    fn the_stored_form_names_the_operation_and_both_digests_and_round_trips() -> Result<(), RequestFingerprintError> {
        let fingerprint = fingerprint(MapRequestKind::QueryTemporal, 0xab, 0xcd);
        let stored = fingerprint.to_string();
        pretty_assertions::assert_eq!(stored, format!("query-temporal:{}:{}", "ab".repeat(32), "cd".repeat(32)));
        pretty_assertions::assert_eq!(stored.parse::<RequestFingerprint>()?, fingerprint);
        Ok(())
    }

    #[test]
    fn malformed_stored_text_is_refused_with_the_reason_it_is_malformed() {
        pretty_assertions::assert_eq!("query-entities".parse::<RequestFingerprint>(), Err(RequestFingerprintError::MissingSeparator));
        pretty_assertions::assert_eq!(
            format!("query-entities:{}", "0".repeat(64)).parse::<RequestFingerprint>(),
            Err(RequestFingerprintError::MissingSeparator)
        );
        assert!(matches!(
            format!("purge:{0}:{0}", "0".repeat(64)).parse::<RequestFingerprint>(),
            Err(RequestFingerprintError::UnknownOperation(_))
        ));
        assert!(matches!(
            format!("retrieve-entity:{}:{}", "0".repeat(63), "0".repeat(64)).parse::<RequestFingerprint>(),
            Err(RequestFingerprintError::Digest(_))
        ));
        pretty_assertions::assert_eq!(
            format!("retrieve-entity:{}:{}", "0".repeat(64), "A".repeat(64)).parse::<RequestFingerprint>(),
            Err(RequestFingerprintError::NonCanonicalDigest)
        );
    }

    #[test]
    fn comparison_distinguishes_same_changed_and_other_operations() {
        let created = fingerprint(MapRequestKind::QueryEntities, 1, 1);
        pretty_assertions::assert_eq!(created.compare(&fingerprint(MapRequestKind::QueryEntities, 1, 1)), RequestComparison::Same);
        pretty_assertions::assert_eq!(created.compare(&fingerprint(MapRequestKind::QueryEntities, 2, 1)), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            created.compare(&fingerprint(MapRequestKind::RetrieveEntity, 2, 2)),
            RequestComparison::DifferentOperation
        );
    }

    /// Clause 5.5.14 holds a follow-up to every input of the creating
    /// request, so the same query under another input is a changed request.
    #[test]
    fn the_same_query_with_other_inputs_is_a_changed_request() {
        let created = fingerprint(MapRequestKind::QueryEntities, 1, 1);
        pretty_assertions::assert_eq!(created.compare(&fingerprint(MapRequestKind::QueryEntities, 1, 2)), RequestComparison::Changed);
        pretty_assertions::assert_eq!(
            created.compare(&fingerprint(MapRequestKind::RetrieveEntity, 1, 2)),
            RequestComparison::DifferentOperation
        );
    }

    /// A purge is held to the creating request's query alone, whatever its
    /// other inputs were.
    #[test]
    fn the_query_alone_compares_without_the_other_inputs() {
        let created = fingerprint(MapRequestKind::QueryEntities, 1, 1);
        pretty_assertions::assert_eq!(
            created.query().compare(&QueryFingerprint::new(MapRequestKind::QueryEntities, [1; 32])),
            RequestComparison::Same
        );
        pretty_assertions::assert_eq!(
            created.query().compare(&QueryFingerprint::new(MapRequestKind::QueryEntities, [2; 32])),
            RequestComparison::Changed
        );
    }
}
