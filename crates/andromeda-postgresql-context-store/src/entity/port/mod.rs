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

/// Map an adapter failure into the entity port's persistence category.
fn persistence(error: ContextStoreError) -> EntityError {
    EntityError::persistence(error.to_string())
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
