//! PostgreSQL source for typed entity spatial-index snapshots.

use crate::entity::{
    mapper::{stored_type_name, stored_uri},
    repo::EntityRepository,
};
use andromeda_entity::{
    error::EntityError,
    port::spatial_snapshot::{EntitySpatialSnapshot, EntitySpatialSnapshotSource},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use async_trait::async_trait;
use tracing::warn;

/// Read the typed columns required to reconstruct default-location index entries.
const LOCATION_SNAPSHOTS_SQL: &str = "SELECT id, types, location::bytea FROM entities WHERE location IS NOT NULL";

#[async_trait]
impl EntitySpatialSnapshotSource for EntityRepository {
    async fn location_snapshots(&self) -> Result<Vec<EntitySpatialSnapshot>, EntityError> {
        let rows = sqlx::query_as::<_, (String, Vec<String>, Vec<u8>)>(LOCATION_SNAPSHOTS_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| EntityError::persistence(format!("spatial snapshot query failed: {error}")))?;

        let mut snapshots = Vec::with_capacity(rows.len());
        for (identifier, entity_types, ewkb_bytes) in rows {
            let entity_id = match stored_uri(identifier.clone()) {
                Ok(entity_id) => entity_id,
                Err(error) => {
                    warn!(entity = %identifier, %error, "spatial snapshot skipped an invalid stored entity identifier");
                    continue;
                }
            };
            let geometry = match Ewkb::try_from(ewkb_bytes.as_slice()).and_then(|ewkb| Geometry::try_from(&ewkb)) {
                Ok(geometry) => geometry,
                Err(error) => {
                    warn!(entity = %identifier, %error, "spatial snapshot skipped an undecodable location");
                    continue;
                }
            };
            let entity_types = entity_types
                .into_iter()
                .map(stored_type_name)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| EntityError::persistence(error.to_string()))?;
            snapshots.push(EntitySpatialSnapshot {
                entity_id,
                entity_types,
                geometry,
            });
        }
        Ok(snapshots)
    }
}

#[cfg(test)]
mod tests {
    use super::LOCATION_SNAPSHOTS_SQL;
    use crate::entity::repo::EntityRepository;
    use andromeda_entity::port::spatial_snapshot::EntitySpatialSnapshotSource;

    fn assert_spatial_source<T: EntitySpatialSnapshotSource>() {}

    #[test]
    fn entity_repository_directly_implements_the_spatial_snapshot_source() {
        assert_spatial_source::<EntityRepository>();
        assert!(LOCATION_SNAPSHOTS_SQL.starts_with("SELECT id, types, location::bytea"));
    }
}
