//! The persisted API of one stored posted query.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The API a stored posted query was posted to, and whose GET binding its
    /// page links are followed on.
    PostedQueryKindCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 0,
        frozen_digest: 0xcbf2_9ce4_8422_2325,
        codes: {
            /// Query Entities, posted as `POST /entityOperations/query`.
            Entities = 1, "entities";
            /// Query Temporal Evolution of Entities, posted as
            /// `POST /temporal/entityOperations/query`.
            Temporal = 2, "temporal";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::posted_query_kind::PostedQueryKindCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(PostedQueryKindCode::ALL.len(), 2);
        assert_eq!(PostedQueryKindCode::Entities.code(), 1);
        assert_eq!(PostedQueryKindCode::Temporal.code(), 2);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(PostedQueryKindCode::try_from(0).is_err());
        assert!(PostedQueryKindCode::try_from(3).is_err());
    }
}
