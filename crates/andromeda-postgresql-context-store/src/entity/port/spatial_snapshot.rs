//! PostgreSQL source of the entity locations an entity spatial index is built
//! and caught up from.
//!
//! Both reads return, in the one statement that reads the rows, the `xmin` of
//! that statement's snapshot. Every transaction below it had finished when the
//! snapshot was taken, so a row it wrote is either in the read or was
//! overwritten by a later one; every row a later transaction writes carries a
//! `written_xid` at or above it. Reading the rows written at or above the
//! previous read's horizon therefore misses no change the previous read did not
//! already hold, whichever broker process committed it.
//!
//! A change read also stops below the `xmax` of its own snapshot. Every row the
//! snapshot sees was written by a transaction below that `xmax`, so the bound
//! drops no change. It drops a row restored from a dump of another cluster,
//! whose `written_xid` may lie ahead of this cluster's counter. The full read
//! that seeds the index holds such a row already, and without the bound every
//! change read would return it again until the counter passed it. A later
//! write restamps the row with this cluster's identifier.

use crate::{
    entity::{
        mapper::{stored_type_name, stored_uri},
        repo::EntityRepository,
    },
    store_fault::{contract_broken, persistence_failure, undecodable},
};
use andromeda_entity::{
    error::EntityError,
    port::spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_postgresql_query::dictionary::names_in_key_order::type_names_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::{cast, typed_null},
    pg_type::PostgresType,
    setting::lift_statement_timeout,
    statement::Statement,
    transaction_id::{as_transaction_id, snapshot_xmax, snapshot_xmin},
};
use async_trait::async_trait;
use sea_query::{Expr, ExprTrait, JoinType, Query, SelectStatement, UnionType};
use tracing::warn;

/// Which entities a location read returns.
#[derive(Clone, Copy, Debug)]
enum LocationFilter {
    /// Every entity that conveys a `location`.
    Located,
    /// Every entity whose search row was written at or above this `xid8`
    /// position and below the `xmax` of the read's snapshot, located or not:
    /// an entity that lost its `location` has to leave the index.
    WrittenSince(i64),
}

/// One row of a location read: the horizon row carries only the horizon, and
/// every entity row carries everything else.
type LocationRow = (Option<i64>, Option<String>, Option<Vec<String>>, Option<Vec<u8>>);

/// The horizon of the statement's snapshot, then one row per entity the filter
/// selects: its identifier, its type IRIs (read back through the type
/// dictionary) and its `location` as EWKB.
///
/// The two are one `UNION ALL` so they share the statement's one snapshot, and
/// the horizon row comes back even when no entity does.
fn locations_statement(filter: LocationFilter) -> SelectStatement {
    let mut entities = Query::select();
    entities
        .expr(typed_null(PostgresType::Bigint))
        .column((Entities::Table, Entities::Id))
        .expr(type_names_of(Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys))))
        .expr(cast(Expr::col((EntitySearch::Table, EntitySearch::Location)), PostgresType::Bytea))
        .from(Entities::Table)
        .join(
            JoinType::Join,
            EntitySearch::Table,
            Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        );
    match filter {
        LocationFilter::Located => entities.and_where(Expr::col((EntitySearch::Table, EntitySearch::Location)).is_not_null()),
        LocationFilter::WrittenSince(position) => entities
            .and_where(Expr::col((EntitySearch::Table, EntitySearch::WrittenXid)).gte(as_transaction_id(Expr::val(position))))
            .and_where(Expr::col((EntitySearch::Table, EntitySearch::WrittenXid)).lt(snapshot_xmax())),
    };
    Query::select()
        .expr(snapshot_xmin())
        .expr(typed_null(PostgresType::Text))
        .expr(typed_null(PostgresType::TextArray))
        .expr(typed_null(PostgresType::Bytea))
        .union(UnionType::All, entities.take())
        .to_owned()
}

impl EntityRepository {
    /// Run one location read, in a transaction of its own.
    async fn location_rows(&self, filter: LocationFilter) -> Result<Vec<LocationRow>, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        match filter {
            LocationFilter::Located => {
                Query::select().expr(lift_statement_timeout()).to_query().execute(&mut *transaction).await?;
            }
            LocationFilter::WrittenSince(_) => {}
        }
        let rows = locations_statement(filter).to_query_as::<LocationRow>().fetch_all(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(rows)
    }

    /// Run one location read and decode its rows.
    ///
    /// Reading every location builds a tenant's index when it opens, which no
    /// request waits for and which takes as long as the tenant is large, so
    /// that read lifts the statement timeout for its own transaction; a
    /// catch-up read is bounded by the writes since the previous one.
    async fn read_locations(&self, filter: LocationFilter) -> Result<LocationRead, EntityError> {
        let rows = self
            .location_rows(filter)
            .await
            .map_err(|error| EntityError::from(persistence_failure(&error)))?;

        let mut horizon = None;
        let mut locations = Vec::with_capacity(rows.len());
        for row in rows {
            match row {
                (Some(position), None, None, None) => {
                    let position = u64::try_from(position).map_err(|error| EntityError::from(undecodable(&error)))?;
                    horizon = Some(ChangeHorizon::new(position));
                }
                (None, Some(identifier), Some(entity_types), location) => {
                    if let Some(location) = decode_location(&identifier, &entity_types, location)? {
                        locations.push(location);
                    }
                }
                _malformed => {
                    return Err(EntityError::from(contract_broken()));
                }
            }
        }
        let horizon = horizon.ok_or_else(|| EntityError::from(contract_broken()))?;
        Ok(LocationRead { locations, horizon })
    }
}

/// Decode one entity row, or `None` for a row whose identifier or geometry the
/// store holds in a form no entity could have been written with.
fn decode_location(identifier: &str, entity_types: &[String], location: Option<Vec<u8>>) -> Result<Option<EntityLocation>, EntityError> {
    let entity_id = match stored_uri(identifier) {
        Ok(entity_id) => entity_id,
        Err(error) => {
            warn!(entity = %identifier, %error, "spatial location read skipped an invalid stored entity identifier");
            return Ok(None);
        }
    };
    let location = match location.map(|bytes| Ewkb::try_from(bytes.as_slice()).and_then(|ewkb| Geometry::try_from(&ewkb))) {
        None => None,
        Some(Ok(geometry)) => Some(geometry),
        Some(Err(error)) => {
            warn!(entity = %identifier, %error, "spatial location read skipped an undecodable location");
            return Ok(None);
        }
    };
    let entity_types = entity_types
        .iter()
        .map(String::as_str)
        .map(stored_type_name)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EntityError::from(persistence_failure(&error)))?;
    Ok(Some(EntityLocation {
        entity_id,
        entity_types,
        location,
    }))
}

#[async_trait]
impl EntitySpatialSnapshotSource for EntityRepository {
    async fn locations(&self) -> Result<LocationRead, EntityError> {
        self.read_locations(LocationFilter::Located).await
    }

    async fn locations_changed_since(&self, horizon: ChangeHorizon) -> Result<LocationRead, EntityError> {
        let position = i64::try_from(horizon.position()).map_err(|error| EntityError::from(PersistenceFailure::Unencodable.record(&error)))?;
        self.read_locations(LocationFilter::WrittenSince(position)).await
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        port::spatial_snapshot::{LocationFilter, locations_statement},
        repo::EntityRepository,
    };
    use andromeda_entity::port::spatial_snapshot::EntitySpatialSnapshotSource;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn assert_spatial_source<T: EntitySpatialSnapshotSource>() {}

    /// The dictionary lookup every read projects the type IRIs through.
    const TYPE_NAMES: &str = r#"ARRAY (SELECT "entity_type_names"."name" FROM "entity_type_names" WHERE "entity_type_names"."entity_type_name_key" = ANY("entity_search"."entity_type_name_keys") ORDER BY array_position("entity_search"."entity_type_name_keys", "entity_type_names"."entity_type_name_key") ASC)"#;

    #[test]
    fn entity_repository_directly_implements_the_spatial_snapshot_source() {
        assert_spatial_source::<EntityRepository>();
    }

    #[test]
    fn a_full_read_returns_the_snapshot_horizon_beside_every_located_entity() {
        let (sql, values) = locations_statement(LocationFilter::Located).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"SELECT pg_snapshot_xmin(pg_current_snapshot())::text::bigint, NULL::text, NULL::text[], NULL::bytea UNION ALL (SELECT NULL::bigint, "entities"."id", {TYPE_NAMES}, "entity_search"."location"::bytea FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key" WHERE "entity_search"."location" IS NOT NULL)"#
            ),
            "{sql}"
        );
        assert_eq!(values.0.len(), 0, "the full read binds nothing: {sql}");
    }

    #[test]
    fn a_change_read_returns_every_entity_written_from_the_horizon_to_its_snapshot_ceiling_located_or_not() {
        let (sql, values) = locations_statement(LocationFilter::WrittenSince(42)).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"SELECT pg_snapshot_xmin(pg_current_snapshot())::text::bigint, NULL::text, NULL::text[], NULL::bytea UNION ALL (SELECT NULL::bigint, "entities"."id", {TYPE_NAMES}, "entity_search"."location"::bytea FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key" WHERE "entity_search"."written_xid" >= ($1::text::xid8) AND "entity_search"."written_xid" < pg_snapshot_xmax(pg_current_snapshot()))"#
            ),
            "{sql}"
        );
        assert_eq!(values.0.len(), 1, "the change read binds its horizon: {sql}");
    }
}
