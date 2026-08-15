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
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use async_trait::async_trait;
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use tracing::warn;

/// Read the typed columns required to reconstruct default-location index
/// entries: the identifier, the type IRIs, and the location geometry as EWKB.
fn location_snapshots_statement() -> SelectStatement {
    Query::select()
        .column(Entities::Id)
        .column(Entities::Types)
        .expr(cast(Expr::col(Entities::Location), PostgresType::Bytea))
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Location).is_not_null())
        .to_owned()
}

#[async_trait]
impl EntitySpatialSnapshotSource for EntityRepository {
    async fn location_snapshots(&self) -> Result<Vec<EntitySpatialSnapshot>, EntityError> {
        let (sql, values) = location_snapshots_statement().build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_as_with::<_, (String, Vec<String>, Vec<u8>), _>(sqlx::AssertSqlSafe(sql.as_str()), values)
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
    use crate::entity::{port::spatial_snapshot::location_snapshots_statement, repo::EntityRepository};
    use andromeda_entity::port::spatial_snapshot::EntitySpatialSnapshotSource;
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    fn assert_spatial_source<T: EntitySpatialSnapshotSource>() {}

    #[test]
    fn entity_repository_directly_implements_the_spatial_snapshot_source() {
        assert_spatial_source::<EntityRepository>();
    }

    #[test]
    fn the_snapshot_reads_the_typed_columns_of_located_entities() {
        let (sql, values) = location_snapshots_statement().build_sqlx(PostgresQueryBuilder);
        assert_eq!(
            sql, r#"SELECT "id", "types", "location"::bytea FROM "entities" WHERE "location" IS NOT NULL"#,
            "{sql}"
        );
        assert_eq!(values.0.0.len(), 0, "the snapshot reads every located entity, binding nothing: {sql}");
    }
}
