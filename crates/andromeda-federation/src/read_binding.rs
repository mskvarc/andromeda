//! Choosing how a forwarded entity query reaches one registration, from the
//! query operations that registration advertises (clauses 4.3.6.1, 4.20, 6.23.1).
//!
//! Clause 4.3.6.1: "registered Context Sources may indicate that they are only
//! willing to respond to a limited subset of API operations. Context Brokers
//! shall respect this". Query Entities has two bindings of the same operation
//! (clause 6.23.1), which Table 4.20-1 names `queryEntity` (the GET binding) and
//! `queryBatch` (the POST binding), so a query is forwarded through whichever of
//! the two the registration advertises.
//!
//! A Retrieve Entity is not a query and is never bound here. Table 4.20-1 names
//! it `retrieveEntity`, an operation of its own, and clause 5.7.1.4 forwards it
//! only to the registrations that "match and support the retrieveEntity
//! operation"; unlike Upsert (clause 5.6.8.4) it spells out no fallback to
//! another operation, so a registration advertising only `queryEntity` or
//! `queryBatch` is not contacted for it.

use crate::operation::ForwardingOperation;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

/// The binding the client's entity query arrived on, which fixes the preference
/// order of the query operations a registration may be contacted through.
///
/// A query prefers to be forwarded through the same operation the client used
/// (clause 6.23.1: the GET and POST bindings are the same operation), falling
/// back to the other query operation when the registration does not advertise
/// it; so a GET query prefers `queryEntity` and a POST query prefers `queryBatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadIntent {
    /// Query entities via GET (clause 5.7.2), which prefers `queryEntity`.
    Query,
    /// Query entities via POST `/entityOperations/query` (clause 6.23), which
    /// prefers `queryBatch`.
    QueryBatch,
}

/// How one registration is contacted for a forwarded entity query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadBinding {
    /// GET `/entities` (queryEntity).
    Query,
    /// POST `/entityOperations/query` (queryBatch).
    BatchQuery,
}

/// The binding a forwarded query uses for `csr`, chosen from the query
/// operations it advertises, or `None` when it advertises neither and so is
/// not contacted.
///
/// A query prefers the operation the client used (clause 6.23.1), so a GET query
/// prefers `queryEntity`, a POST query prefers `queryBatch`, and falls back to
/// the other. `redirectionOps` and `retrieveOps` advertise `queryEntity` but not
/// `queryBatch` (Table 4.20-2), so a client `queryBatch` to such a registration
/// is sent as a GET `queryEntity`.
pub(crate) fn read_binding_for(intent: ReadIntent, csr: &ContextSourceRegistration) -> Option<ReadBinding> {
    let (preferred, fallback) = match intent {
        ReadIntent::Query => (
            (ForwardingOperation::QueryEntity, ReadBinding::Query),
            (ForwardingOperation::QueryBatch, ReadBinding::BatchQuery),
        ),
        ReadIntent::QueryBatch => (
            (ForwardingOperation::QueryBatch, ReadBinding::BatchQuery),
            (ForwardingOperation::QueryEntity, ReadBinding::Query),
        ),
    };
    [preferred, fallback]
        .into_iter()
        .find_map(|(operation, binding)| operation.matches(csr).then_some(binding))
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
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["federationOps"])), Some(ReadBinding::Query));
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["retrieveEntity"])), None);
        assert_eq!(read_binding_for(ReadIntent::Query, &registration(&["updateOps"])), None);
    }

    /// Clause 6.23.1: a POST query is the same operation on the batch binding, so
    /// it prefers `queryBatch` and is sent as a GET `queryEntity` only when the
    /// registration does not advertise `queryBatch` (e.g. redirectionOps).
    #[test]
    fn a_post_query_prefers_batch_then_query() {
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["federationOps"])),
            Some(ReadBinding::BatchQuery)
        );
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["queryBatch"])),
            Some(ReadBinding::BatchQuery)
        );
        // redirectionOps has queryEntity but not queryBatch, so a POST query is
        // sent as a GET queryEntity.
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["redirectionOps"])),
            Some(ReadBinding::Query)
        );
        assert_eq!(
            read_binding_for(ReadIntent::QueryBatch, &registration(&["queryEntity"])),
            Some(ReadBinding::Query)
        );
        assert_eq!(read_binding_for(ReadIntent::QueryBatch, &registration(&["retrieveEntity"])), None);
        assert_eq!(read_binding_for(ReadIntent::QueryBatch, &registration(&["updateOps"])), None);
    }
}
