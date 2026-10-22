//! Direct PostgreSQL implementations of entity policy ports.

/// Deletion outlines and the deletion-watcher probe.
mod deletion_outline;
/// Clause 5.7 discovery reads.
mod discovery;
/// The types one entity carries, read from its search row.
mod entity_types;
/// Clause 4.22 expiry candidates.
mod expiry;
/// Whole entity payloads read a page at a time.
mod payload_pages;
/// Compiled entity-query execution.
mod query;
/// Entity payload and snapshot reads.
mod reader;
/// Typed entity spatial snapshots.
mod spatial_snapshot;

use crate::{error::ContextStoreError, store_fault::persistence_failure};
use andromeda_entity::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_postgresql_collation::provisioned_collations::CollationProvisioningError;

/// Map an adapter failure into the category the entity policy answers it in.
///
/// Everything the adapter can fail at is the broker's own, with one exception:
/// clauses 5.7.2.4 and 5.7.4.4 say that if "a preferred collation setting [...]
/// does not conform to a valid ICU collation (see IETF RFC 6067) then an error
/// of type `BadRequestData` shall be raised", and whether ICU holds a collation
/// for a locale is a question only the database can answer. A refusal of the
/// client's own `collation` is therefore the client's fault, not a 500. So is
/// a regular expression of the request the database cannot compile (SQLSTATE
/// 2201B): the pattern is the client's.
fn classify(error: &ContextStoreError) -> EntityError {
    match error {
        ContextStoreError::Collation(CollationProvisioningError::NotAnIcuCollation { collation, .. }) => {
            EntityError::Validation(EntityValidationReason::Collation {
                collation: collation.clone(), // clone: the refusal owns the collation the client wrote
            })
        }
        ContextStoreError::InvalidRegex(_) => EntityError::Validation(EntityValidationReason::UnusablePattern),
        ContextStoreError::Collation(CollationProvisioningError::Statement { .. })
        | ContextStoreError::Saturated
        | ContextStoreError::Database(_)
        | ContextStoreError::Encoding(_)
        | ContextStoreError::CopyField(_)
        | ContextStoreError::UniqueViolation(_)
        | ContextStoreError::Render(_)
        | ContextStoreError::NameLookup(_)
        | ContextStoreError::EntityTypes(_)
        | ContextStoreError::ContextMapper(_)
        | ContextStoreError::Migration(_)
        | ContextStoreError::SchemaContract(_)
        | ContextStoreError::BranchVersion(_)
        | ContextStoreError::StatementBuild(_)
        | ContextStoreError::ChangeEventRecord(_)
        | ContextStoreError::StateRecord(_)
        | ContextStoreError::OrdinalOutOfRange(_)
        | ContextStoreError::StoredRegistration(_)
        | ContextStoreError::StoredSubscription(_)
        | ContextStoreError::StoredSnapshot(_)
        | ContextStoreError::UnencodableSnapshot(_)
        | ContextStoreError::StoredEntity(_)
        | ContextStoreError::StoredEntityMap(_)
        | ContextStoreError::StoredTenantStatus(_)
        | ContextStoreError::CatalogueIdentityGeneration(_)
        | ContextStoreError::Invariant(_)
        | ContextStoreError::DatabaseUrlUnnamed => EntityError::from(persistence_failure(error)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{port::classify, repo::EntityRepository},
        error::ContextStoreError,
    };
    use andromeda_entity::{
        entity_validation_reason::EntityValidationReason,
        error::EntityError,
        port::{
            discovery::EntityDiscoveryStore,
            expiry::EntityExpirySource,
            payload_pages::EntityPayloadPages,
            query::EntityQueryStore,
            reader::EntityReader,
            spatial_snapshot::EntitySpatialSnapshotSource,
        },
    };
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;

    fn assert_reader<T: EntityReader>() {}
    fn assert_query_store<T: EntityQueryStore>() {}
    fn assert_discovery_store<T: EntityDiscoveryStore>() {}
    fn assert_expiry_source<T: EntityExpirySource>() {}
    fn assert_payload_pages<T: EntityPayloadPages>() {}
    fn assert_spatial_snapshot_source<T: EntitySpatialSnapshotSource>() {}

    #[test]
    fn entity_repository_implements_every_focused_entity_port() {
        assert_reader::<EntityRepository>();
        assert_query_store::<EntityRepository>();
        assert_discovery_store::<EntityRepository>();
        assert_expiry_source::<EntityRepository>();
        assert_payload_pages::<EntityRepository>();
        assert_spatial_snapshot_source::<EntityRepository>();
    }

    /// SQLSTATE 2201B: the request's own regular expression is refused as
    /// `BadRequestData`, as on the temporal path.
    #[test]
    fn a_regular_expression_the_database_cannot_compile_is_the_requests_pattern_refused() {
        let refused = classify(&ContextStoreError::from(
            FakeDatabaseError::new("invalid regular expression", "2201B").into_sqlx(),
        ));
        assert!(matches!(refused, EntityError::Validation(EntityValidationReason::UnusablePattern)));
    }

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
        assert!(matches!(
            classify(&ContextStoreError::from(sqlx::Error::PoolTimedOut)),
            EntityError::StoreSaturated
        ));
        assert!(!matches!(
            classify(&ContextStoreError::from(sqlx::Error::RowNotFound)),
            EntityError::StoreSaturated
        ));
        // A query resolves its names before its own statement runs, and a
        // saturated pool there is the same backpressure.
        assert!(matches!(
            classify(&ContextStoreError::from(NameLookupError::from(sqlx::Error::PoolTimedOut))),
            EntityError::StoreSaturated
        ));
    }
}
