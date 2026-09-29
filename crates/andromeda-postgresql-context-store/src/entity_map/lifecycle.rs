//! The statements over the `entity_maps` control record: create, read, touch
//! the expiry, delete, and sweep. Everything here names a map by its URI; the
//! surrogate `entity_map_key` is issued by the database and resolved in-statement.

use crate::entity_map::persistence_error;
use andromeda_entity_map::{error::EntityMapError, store::EntityMapMutation};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_postgresql_schema::entity_map::EntityMaps;
use andromeda_postgresql_seaquery::clock::now;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, PgPool, Row};

/// The control record of a live map, as read back from `entity_maps`.
pub(crate) struct MapControlRecord {
    /// The map's expiry.
    pub(crate) expires_at: DateTime<Utc>,
    /// The stored `<operation>:<sha256 hex>` fingerprint of the creating
    /// request, absent on maps created before it was recorded.
    pub(crate) request_fingerprint: Option<String>,
}

/// The `SELECT` resolving a map's URI to the surrogate the child tables carry.
fn entity_map_key_lookup(id: SimpleExpr) -> SelectStatement {
    let mut lookup = Query::select();
    lookup
        .column((EntityMaps::Table, EntityMaps::EntityMapKey))
        .from(EntityMaps::Table)
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id));
    lookup
}

/// `(SELECT entity_maps.entity_map_key FROM entity_maps WHERE entity_maps.id = <id>)`.
pub(crate) fn entity_map_key_of(id: SimpleExpr) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(entity_map_key_lookup(id).into_sub_query_statement()))
}

/// The same lookup restricted to a map that is still live at the database
/// clock: an expired map resolves to NULL, so a statement keyed on it matches
/// nothing rather than reaching into a map clause 5.5.14 says yields no
/// inference.
pub(crate) fn live_entity_map_key_of(id: SimpleExpr) -> SimpleExpr {
    let mut lookup = entity_map_key_lookup(id);
    lookup.and_where(live());
    SimpleExpr::SubQuery(None, Box::new(lookup.into_sub_query_statement()))
}

/// The predicate selecting maps that are still live at the database clock.
fn live() -> SimpleExpr {
    Expr::col((EntityMaps::Table, EntityMaps::ExpiresAt)).gt(now())
}

/// Map a rows-affected count to the policy-level outcome.
const fn mutation_outcome(rows_affected: u64) -> EntityMapMutation {
    match rows_affected {
        0 => EntityMapMutation::Missing,
        1.. => EntityMapMutation::Applied,
    }
}

/// Remove any previous map with the same identifier; the children go with it
/// through the cascading foreign keys.
pub(crate) async fn delete_map_row(connection: &mut PgConnection, id: &Identifier) -> Result<EntityMapMutation, EntityMapError> {
    let (sql, values) = Query::delete()
        .from_table(EntityMaps::Table)
        .and_where(Expr::col(EntityMaps::Id).eq(id.as_str()))
        .build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(connection)
        .await
        .map_err(persistence_error)?;
    Ok(mutation_outcome(result.rows_affected()))
}

/// Write the control record: identifier, expiry and the creating request's
/// fingerprint.
pub(crate) async fn insert_map_row(connection: &mut PgConnection, map: &EntityMap) -> Result<(), EntityMapError> {
    let fingerprint = map.request_fingerprint.as_ref().map(ToString::to_string);
    let mut insert = Query::insert();
    insert
        .into_table(EntityMaps::Table)
        .columns([EntityMaps::Id, EntityMaps::ExpiresAt, EntityMaps::RequestFingerprint])
        .values([Expr::val(map.id.as_str()), Expr::val(map.expires_at), Expr::val(fingerprint)])
        .map_err(persistence_error)?;
    let (sql, values) = insert.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(connection)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// The control record of a live map, or `None` when the map is absent or expired.
pub(crate) async fn read_live_control(pool: &PgPool, id: &Identifier) -> Result<Option<MapControlRecord>, EntityMapError> {
    let (sql, values) = Query::select()
        .column((EntityMaps::Table, EntityMaps::ExpiresAt))
        .column((EntityMaps::Table, EntityMaps::RequestFingerprint))
        .from(EntityMaps::Table)
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id.as_str()))
        .and_where(live())
        .build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;
    row.map(|row| {
        Ok(MapControlRecord {
            expires_at: row.try_get::<DateTime<Utc>, _>("expires_at").map_err(persistence_error)?,
            request_fingerprint: row.try_get::<Option<String>, _>("request_fingerprint").map_err(persistence_error)?,
        })
    })
    .transpose()
}

/// Move a live map's expiry.
pub(crate) async fn update_expiry(pool: &PgPool, id: &Identifier, expires_at: DateTime<Utc>) -> Result<EntityMapMutation, EntityMapError> {
    let (sql, values) = Query::update()
        .table(EntityMaps::Table)
        .value(EntityMaps::ExpiresAt, expires_at)
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id.as_str()))
        .and_where(live())
        .build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(mutation_outcome(result.rows_affected()))
}

/// Remove a live map.
pub(crate) async fn delete_live(pool: &PgPool, id: &Identifier) -> Result<EntityMapMutation, EntityMapError> {
    let (sql, values) = Query::delete()
        .from_table(EntityMaps::Table)
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::Id)).eq(id.as_str()))
        .and_where(live())
        .build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(mutation_outcome(result.rows_affected()))
}

/// Remove every map due at or before `cutoff`.
pub(crate) async fn delete_expired(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, EntityMapError> {
    let (sql, values) = Query::delete()
        .from_table(EntityMaps::Table)
        .and_where(Expr::col((EntityMaps::Table, EntityMaps::ExpiresAt)).lte(cutoff))
        .build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use crate::entity_map::lifecycle::{entity_map_key_of, mutation_outcome};
    use andromeda_entity_map::store::EntityMapMutation;
    use andromeda_postgresql_seaquery::placeholder::external_placeholder;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn the_key_lookup_is_a_qualified_scalar_subquery_on_the_unique_identifier() {
        let sql = Query::select()
            .expr(Expr::col(Alias::new("entity_id")))
            .from(Alias::new("entity_map_entries"))
            .and_where(Expr::col(Alias::new("entity_map_key")).eq(entity_map_key_of(external_placeholder(1))))
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"(SELECT "entity_maps"."entity_map_key" FROM "entity_maps" WHERE "entity_maps"."id" = ($1))"#),
            "{sql}"
        );
    }

    #[test]
    fn affected_row_counts_map_to_mutation_outcomes() {
        assert_eq!(mutation_outcome(0), EntityMapMutation::Missing);
        assert_eq!(mutation_outcome(1), EntityMapMutation::Applied);
    }
}
