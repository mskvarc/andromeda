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
use andromeda_postgresql_query::entity_type_names::type_names_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use async_trait::async_trait;
use sea_query::{Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use tracing::warn;

/// Read the typed columns required to reconstruct default-location index
/// entries: the identifier, the type IRIs (read back through the type
/// dictionary), and the location geometry as EWKB.
fn location_snapshots_statement() -> SelectStatement {
    Query::select()
        .column((Entities::Table, Entities::Id))
        .expr(type_names_of(Expr::col((EntitySearch::Table, EntitySearch::TypeIds))))
        .expr(cast(Expr::col((EntitySearch::Table, EntitySearch::Location)), PostgresType::Bytea))
        .from(Entities::Table)
        .join(
            JoinType::Join,
            EntitySearch::Table,
            Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        )
        .and_where(Expr::col((EntitySearch::Table, EntitySearch::Location)).is_not_null())
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
            let entity_id = match stored_uri(&identifier) {
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
                .iter()
                .map(String::as_str)
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
            sql,
            r#"SELECT "entities"."id", ARRAY(SELECT "entity_type_names"."name" FROM "entity_type_names" WHERE "entity_type_names"."id" = ANY("entity_search"."type_ids") ORDER BY array_position("entity_search"."type_ids", "entity_type_names"."id") ASC), "entity_search"."location"::bytea FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key" WHERE "entity_search"."location" IS NOT NULL"#,
            "{sql}"
        );
        assert_eq!(values.0.0.len(), 0, "the snapshot reads every located entity, binding nothing: {sql}");
    }
}
