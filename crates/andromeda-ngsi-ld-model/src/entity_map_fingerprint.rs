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

/// The operation that created an `EntityMap` (clause 5.5.14 lists the four).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// The bytes of each digest of a map's request record, a SHA-256 output.
pub const REQUEST_DIGEST_BYTES: usize = 32;

/// The digest of what a request's compiled query selects, tagged with the
/// operation it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryFingerprint {
    /// The operation the query belongs to.
    kind: MapRequestKind,
    /// The SHA-256 over the canonical identity of the compiled query.
    digest: [u8; REQUEST_DIGEST_BYTES],
}

impl QueryFingerprint {
    /// A query fingerprint of `kind` over an already computed digest.
    #[must_use]
    pub const fn new(kind: MapRequestKind, digest: [u8; REQUEST_DIGEST_BYTES]) -> QueryFingerprint {
        QueryFingerprint { kind, digest }
    }

    /// The operation the query belongs to.
    #[must_use]
    pub const fn kind(&self) -> MapRequestKind {
        self.kind
    }

    /// The SHA-256 over the canonical identity of the compiled query.
    #[must_use]
    pub const fn digest(&self) -> &[u8; REQUEST_DIGEST_BYTES] {
        &self.digest
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestFingerprint {
    /// What the compiled query selects, tagged with the creating operation.
    query: QueryFingerprint,
    /// The SHA-256 over the canonical record of the request's other inputs.
    inputs: [u8; REQUEST_DIGEST_BYTES],
}

impl RequestFingerprint {
    /// A fingerprint of the request whose query is `query` and whose other
    /// inputs digest to `inputs`.
    #[must_use]
    pub const fn new(query: QueryFingerprint, inputs: [u8; REQUEST_DIGEST_BYTES]) -> RequestFingerprint {
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

    /// The SHA-256 over the canonical record of the request's other inputs.
    #[must_use]
    pub const fn inputs(&self) -> &[u8; REQUEST_DIGEST_BYTES] {
        &self.inputs
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

#[cfg(test)]
mod tests {
    use crate::entity_map_fingerprint::{MapRequestKind, QueryFingerprint, RequestComparison, RequestFingerprint};

    fn fingerprint(kind: MapRequestKind, query: u8, inputs: u8) -> RequestFingerprint {
        RequestFingerprint::new(QueryFingerprint::new(kind, [query; 32]), [inputs; 32])
    }

    #[test]
    fn the_digests_are_the_bytes_the_fingerprint_was_built_from() {
        let fingerprint = fingerprint(MapRequestKind::QueryTemporal, 0xab, 0xcd);
        pretty_assertions::assert_eq!(fingerprint.kind(), MapRequestKind::QueryTemporal);
        pretty_assertions::assert_eq!(fingerprint.query().digest(), &[0xab; 32]);
        pretty_assertions::assert_eq!(fingerprint.inputs(), &[0xcd; 32]);
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
