//! Direct PostgreSQL implementations of entity policy ports.

/// Clause 5.7 discovery reads.
mod discovery;
/// Clause 4.22 expiry candidates.
mod expiry;
/// Compiled entity-query execution.
mod query;
/// Entity payload and snapshot reads.
mod reader;
/// Typed entity spatial snapshots.
mod spatial_snapshot;

use crate::error::ContextStoreError;
use andromeda_entity::error::EntityError;
use andromeda_postgresql_collation::provisioned_collations::CollationProvisioningError;

/// Map an adapter failure into the category the entity policy answers it in.
///
/// Everything the adapter can fail at is the broker's own, with one exception:
/// clauses 5.7.2.4 and 5.7.4.4 say that if "a preferred collation setting [...]
/// does not conform to a valid ICU collation (see IETF RFC 6067) then an error
/// of type `BadRequestData` shall be raised", and whether ICU holds a collation
/// for a locale is a question only the database can answer. A refusal of the
/// client's own `collation` is therefore the client's fault, not a 500.
fn classify(error: &ContextStoreError) -> EntityError {
    match error {
        ContextStoreError::Collation(CollationProvisioningError::NotAnIcuCollation { collation, .. }) => {
            EntityError::validation(format!("'{collation}' does not conform to a valid ICU collation"))
        }
        ContextStoreError::Collation(CollationProvisioningError::Statement { .. })
        | ContextStoreError::Database(_)
        | ContextStoreError::Encoding(_)
        | ContextStoreError::CopyField(_)
        | ContextStoreError::UniqueViolation(_)
        | ContextStoreError::InvalidRegex(_)
        | ContextStoreError::Render(_)
        | ContextStoreError::NameLookup(_)
        | ContextStoreError::EntityTypes(_)
        | ContextStoreError::ContextMapper(_)
        | ContextStoreError::Migration(_)
        | ContextStoreError::SchemaContract(_)
        | ContextStoreError::Internal(_) => EntityError::persistence(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::repo::EntityRepository;
    use andromeda_entity::port::{
        discovery::EntityDiscoveryStore,
        expiry::EntityExpirySource,
        query::EntityQueryStore,
        reader::EntityReader,
        spatial_snapshot::EntitySpatialSnapshotSource,
    };

    fn assert_reader<T: EntityReader>() {}
    fn assert_query_store<T: EntityQueryStore>() {}
    fn assert_discovery_store<T: EntityDiscoveryStore>() {}
    fn assert_expiry_source<T: EntityExpirySource>() {}
    fn assert_spatial_snapshot_source<T: EntitySpatialSnapshotSource>() {}

    #[test]
    fn entity_repository_implements_every_focused_entity_port() {
        assert_reader::<EntityRepository>();
        assert_query_store::<EntityRepository>();
        assert_discovery_store::<EntityRepository>();
        assert_expiry_source::<EntityRepository>();
        assert_spatial_snapshot_source::<EntityRepository>();
    }
}
