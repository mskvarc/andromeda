//! The persisted operation that created one stored Entity Map.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The operation whose request created a stored Entity Map, which a
    /// follow-up request is held to.
    EntityMapRequestKindCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 0,
        frozen_digest: 0xcbf2_9ce4_8422_2325,
        codes: {
            /// Query Entities.
            QueryEntities = 1, "query-entities";
            /// Retrieve Entity.
            RetrieveEntity = 2, "retrieve-entity";
            /// Query Temporal Evolution of Entities.
            QueryTemporal = 3, "query-temporal";
            /// Retrieve Temporal Evolution of an Entity.
            RetrieveTemporal = 4, "retrieve-temporal";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map_request_kind::EntityMapRequestKindCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(EntityMapRequestKindCode::ALL.len(), 4);
        assert_eq!(EntityMapRequestKindCode::QueryEntities.code(), 1);
        assert_eq!(EntityMapRequestKindCode::RetrieveEntity.code(), 2);
        assert_eq!(EntityMapRequestKindCode::QueryTemporal.code(), 3);
        assert_eq!(EntityMapRequestKindCode::RetrieveTemporal.code(), 4);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(EntityMapRequestKindCode::try_from(0).is_err());
        assert!(EntityMapRequestKindCode::try_from(5).is_err());
    }
}
