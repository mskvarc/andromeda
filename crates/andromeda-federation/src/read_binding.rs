//! Choosing how a forwarded current-entity read reaches one registration, from
//! the operations that registration advertises (clause 4.3.6.1, Table 4.20-2).
//!
//! Clause 4.3.6.1: "registered Context Sources may indicate that they are only
//! willing to respond to a limited subset of API operations. Context Brokers
//! shall respect this". A registration advertising only `queryEntity` (not
//! `retrieveEntity`), or only `queryBatch` (neither), must still be contacted for
//! a read the client made, through an operation it does allow: a GET query, or a
//! POST batch query. The alternative -- forwarding the client's own operation
//! regardless -- reaches a source that never registered it, so the source is
//! never contacted and its data is lost.

use crate::operation::ForwardingOperation;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

/// The shape of the client's current-entity read, which fixes the preference
/// order of the operations a registration may be contacted through.
///
/// A query prefers to be forwarded through the same operation the client used
/// (clause 6.23.1: the GET and POST bindings are the same operation), falling
/// back to the other query operation when the registration does not advertise
/// it; so a GET query prefers `queryEntity` and a POST query prefers `queryBatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadIntent {
    /// Retrieve one entity by id (clause 5.7.1).
    RetrieveById,
    /// Query entities via GET (clause 5.7.2), which prefers `queryEntity`.
    Query,
    /// Query entities via POST `/entityOperations/query` (clause 6.23), which
    /// prefers `queryBatch`.
    QueryBatch,
}

/// How one registration is contacted for a forwarded current-entity read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadBinding {
    /// GET `/entities/{id}` -- retrieveEntity.
    Retrieve,
    /// GET `/entities` -- queryEntity.
    Query,
    /// POST `/entityOperations/query` -- queryBatch.
    BatchQuery,
}

/// The binding a forwarded read uses for `csr`, chosen from the operations it
/// advertises, or `None` when it advertises no read operation and so is not
/// contacted.
///
/// Table 4.20-2: a retrieve-by-id prefers `retrieveEntity`, then falls back to a
/// GET `queryEntity` and then a POST `queryBatch`. A query prefers the operation
/// the client used (clause 6.23.1) -- a GET query prefers `queryEntity`, a POST
/// query prefers `queryBatch` -- and falls back to the other. `redirectionOps`
/// and `retrieveOps` advertise `queryEntity` but not `queryBatch`, so a client
/// `queryBatch` to such a registration is downgraded to a GET `queryEntity`.
pub(crate) fn read_binding_for(intent: ReadIntent, csr: &ContextSourceRegistration) -> Option<ReadBinding> {
    let advertises = |operation: ForwardingOperation| operation.matches(csr);
    let query = || {
        if advertises(ForwardingOperation::QueryEntity) {
            Some(ReadBinding::Query)
        } else if advertises(ForwardingOperation::QueryBatch) {
            Some(ReadBinding::BatchQuery)
        } else {
            None
        }
    };
    match intent {
        ReadIntent::RetrieveById => {
            if advertises(ForwardingOperation::RetrieveEntity) {
                Some(ReadBinding::Retrieve)
            } else {
                query()
            }
        }
        ReadIntent::Query => query(),
        ReadIntent::QueryBatch => {
            if advertises(ForwardingOperation::QueryBatch) {
                Some(ReadBinding::BatchQuery)
            } else if advertises(ForwardingOperation::QueryEntity) {
                Some(ReadBinding::Query)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::read_binding::{ReadBinding, ReadIntent, read_binding_for};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::json;

    fn registration(operations: &[&str]) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "operations": operations,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// Clause 4.3.6.1 + Table 4.20-2: a read is bound to the most direct
    /// operation the registration advertises, falling back to a query and then a
    /// batch query.
    #[test]
    fn a_retrieve_prefers_retrieve_then_query_then_batch() {
        assert_eq!(
            read_binding_for(ReadIntent::RetrieveById, &registration(&["retrieveEntity"])),
            Some(ReadBinding::Retrieve)
        );
        assert_eq!(
            read_binding_for(ReadIntent::RetrieveById, &registration(&["queryEntity"])),
            Some(ReadBinding::Query)
        );
        assert_eq!(
            read_binding_for(ReadIntent::RetrieveById, &registration(&["queryBatch"])),
            Some(ReadBinding::BatchQuery)
        );
        // redirectionOps advertises retrieveEntity, so a retrieve stays a retrieve.
        assert_eq!(
            read_binding_for(ReadIntent::RetrieveById, &registration(&["redirectionOps"])),
            Some(ReadBinding::Retrieve)
        );
        // federationOps advertises all three read operations.
        assert_eq!(
            read_binding_for(ReadIntent::RetrieveById, &registration(&["federationOps"])),
            Some(ReadBinding::Retrieve)
        );
        assert_eq!(read_binding_for(ReadIntent::RetrieveById, &registration(&["updateOps"])), None);
    }

    #[test]
    fn a_get_query_prefers_query_then_batch() {
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["queryEntity"])), Some(ReadBinding::Query));
        assert_eq!(
            read_binding_for(ReadIntent::Query, &registration(&["queryBatch"])),
            Some(ReadBinding::BatchQuery)
        );
        // redirectionOps has queryEntity but not queryBatch, so a query is a query.
        assert_eq!(
            read_binding_for(ReadIntent::Query, &registration(&["redirectionOps"])),
            Some(ReadBinding::Query)
        );
        // federationOps advertises both, and a GET query prefers queryEntity.
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["federationOps"])), Some(ReadBinding::Query));
        // retrieveEntity alone cannot serve a general query.
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["retrieveEntity"])), None);
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["updateOps"])), None);
    }

    /// Clause 6.23.1: a POST query is the same operation on the batch binding, so
    /// it prefers `queryBatch` and downgrades to a GET `queryEntity` only when the
    /// registration does not advertise `queryBatch` (e.g. redirectionOps).
    #[test]
    fn a_post_query_prefers_batch_then_query() {
        // federationOps advertises both; a POST query prefers queryBatch.
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["federationOps"])),
            Some(ReadBinding::BatchQuery)
        );
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["queryBatch"])),
            Some(ReadBinding::BatchQuery)
        );
        // redirectionOps has queryEntity but not queryBatch, so a POST query is
        // downgraded to a GET queryEntity.
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["redirectionOps"])),
            Some(ReadBinding::Query)
        );
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["queryEntity"])),
            Some(ReadBinding::Query)
        );
        assert_eq!(read_binding_for(ReadIntent::QueryBatch, &registration(&["updateOps"])), None);
    }
}
