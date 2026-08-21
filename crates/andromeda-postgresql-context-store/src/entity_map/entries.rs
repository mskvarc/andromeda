//! The statements over a map's `entity_map_entries` and `entity_map_links`.
//!
//! Both are written in bulk through `UNNEST`, so a map over thousands of
//! entities is one statement per child table rather than one per row.

use crate::entity_map::{lifecycle::map_key_of, persistence_error};
use andromeda_entity_map::error::EntityMapError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_postgresql_schema::entity_map::{EntityMapEntries, EntityMapLinks};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, Order, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, PgPool, Row};

/// The `UNNEST` staging alias a batch of child rows is expanded under.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// One `(position, entity_id, source)` row of the entries table.
struct EntryRow<'a> {
    /// The order the pair was produced in.
    position: i32,
    /// The entity.
    entity_id: &'a str,
    /// The source token.
    source: String,
}

/// The bulk insert of a map's entries: the map's URI bound as `$1`, the
/// positions, entity ids and sources as `$2`/`$3`/`$4`.
pub(crate) fn entries_insert_sql() -> Result<String, EntityMapError> {
    let columns = [
        UnnestColumn::new(EntityMapEntries::Position, PostgresType::IntegerArray),
        UnnestColumn::new(EntityMapEntries::EntityId, PostgresType::TextArray),
        UnnestColumn::new(EntityMapEntries::Source, PostgresType::TextArray),
    ];
    let call = unnest_call([
        (&columns[0], external_placeholder(2)),
        (&columns[1], external_placeholder(3)),
        (&columns[2], external_placeholder(4)),
    ]);
    let mut select = Query::select();
    select
        .expr(map_key_of(external_placeholder(1)))
        .expr(Expr::col((D, EntityMapEntries::Position)))
        .expr(Expr::col((D, EntityMapEntries::EntityId)))
        .expr(Expr::col((D, EntityMapEntries::Source)))
        .from_function(call, D);
    let mut insert = Query::insert();
    insert.into_table(EntityMapEntries::Table).columns([
        EntityMapEntries::MapKey.into_iden(),
        EntityMapEntries::Position.into_iden(),
        EntityMapEntries::EntityId.into_iden(),
        EntityMapEntries::Source.into_iden(),
    ]);
    insert.select_from(select).map_err(persistence_error)?;
    Ok(apply_column_alias(insert.to_string(PostgresQueryBuilder), &D, &columns))
}

/// The bulk insert of a map's links: the map's URI bound as `$1`, the
/// registration ids and linked map ids as `$2`/`$3`.
pub(crate) fn links_insert_sql() -> Result<String, EntityMapError> {
    let columns = [
        UnnestColumn::new(EntityMapLinks::RegistrationId, PostgresType::TextArray),
        UnnestColumn::new(EntityMapLinks::LinkedMapId, PostgresType::TextArray),
    ];
    let call = unnest_call([(&columns[0], external_placeholder(2)), (&columns[1], external_placeholder(3))]);
    let mut select = Query::select();
    select
        .expr(map_key_of(external_placeholder(1)))
        .expr(Expr::col((D, EntityMapLinks::RegistrationId)))
        .expr(Expr::col((D, EntityMapLinks::LinkedMapId)))
        .from_function(call, D);
    let mut insert = Query::insert();
    insert.into_table(EntityMapLinks::Table).columns([
        EntityMapLinks::MapKey.into_iden(),
        EntityMapLinks::RegistrationId.into_iden(),
        EntityMapLinks::LinkedMapId.into_iden(),
    ]);
    insert.select_from(select).map_err(persistence_error)?;
    Ok(apply_column_alias(insert.to_string(PostgresQueryBuilder), &D, &columns))
}

/// Write a map's entries and links.
pub(crate) async fn insert_children(connection: &mut PgConnection, map: &EntityMap) -> Result<(), EntityMapError> {
    let rows = map
        .entity_map
        .iter()
        .flat_map(|(entity_id, sources)| sources.iter().map(move |source| (entity_id, source)))
        .enumerate()
        .map(|(position, (entity_id, source))| {
            Ok(EntryRow {
                position: i32::try_from(position).map_err(persistence_error)?,
                entity_id: entity_id.as_str(),
                source: source.to_string(),
            })
        })
        .collect::<Result<Vec<_>, EntityMapError>>()?;
    if !rows.is_empty() {
        let positions: Vec<i32> = rows.iter().map(|row| row.position).collect();
        let entity_ids: Vec<&str> = rows.iter().map(|row| row.entity_id).collect();
        let sources: Vec<&str> = rows.iter().map(|row| row.source.as_str()).collect();
        let sql = entries_insert_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(map.id.as_str())
            .bind(&positions)
            .bind(&entity_ids)
            .bind(&sources)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?;
    }
    if !map.linked_maps.is_empty() {
        let registrations: Vec<&str> = map.linked_maps.keys().map(Uri::as_str).collect();
        let linked: Vec<&str> = map.linked_maps.values().map(Uri::as_str).collect();
        let sql = links_insert_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(map.id.as_str())
            .bind(&registrations)
            .bind(&linked)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?;
    }
    Ok(())
}

/// A map's `(entity_id, source)` pairs in the order they were produced.
pub(crate) async fn read_entries(pool: &PgPool, id: &Uri) -> Result<Vec<(String, String)>, EntityMapError> {
    let (sql, values) = Query::select()
        .column(EntityMapEntries::EntityId)
        .column(EntityMapEntries::Source)
        .from(EntityMapEntries::Table)
        .and_where(Expr::col(EntityMapEntries::MapKey).eq(map_key_of(Expr::val(id.as_str()))))
        .order_by(EntityMapEntries::Position, Order::Asc)
        .build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("entity_id").map_err(persistence_error)?,
                row.try_get("source").map_err(persistence_error)?,
            ))
        })
        .collect()
}

/// A map's `(registration_id, linked_map_id)` pairs.
pub(crate) async fn read_links(pool: &PgPool, id: &Uri) -> Result<Vec<(String, String)>, EntityMapError> {
    let (sql, values) = Query::select()
        .column(EntityMapLinks::RegistrationId)
        .column(EntityMapLinks::LinkedMapId)
        .from(EntityMapLinks::Table)
        .and_where(Expr::col(EntityMapLinks::MapKey).eq(map_key_of(Expr::val(id.as_str()))))
        .order_by(EntityMapLinks::RegistrationId, Order::Asc)
        .build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("registration_id").map_err(persistence_error)?,
                row.try_get("linked_map_id").map_err(persistence_error)?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::entity_map::entries::{entries_insert_sql, links_insert_sql};

    #[test]
    fn the_entries_insert_stages_three_arrays_under_the_resolved_map_key() {
        let sql = entries_insert_sql().expect("builds");
        assert!(
            sql.contains(r#"INSERT INTO "entity_map_entries" ("map_key", "position", "entity_id", "source")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "entity_maps"."map_key" FROM "entity_maps" WHERE "entity_maps"."id" = ($1))"#),
            "{sql}"
        );
        assert!(
            sql.contains("UNNEST($2::integer[], $3::text[], $4::text[]) AS d(position, entity_id, source)"),
            "{sql}"
        );
    }

    #[test]
    fn the_links_insert_stages_two_arrays_under_the_resolved_map_key() {
        let sql = links_insert_sql().expect("builds");
        assert!(
            sql.contains(r#"INSERT INTO "entity_map_links" ("map_key", "registration_id", "linked_map_id")"#),
            "{sql}"
        );
        assert!(sql.contains("UNNEST($2::text[], $3::text[]) AS d(registration_id, linked_map_id)"), "{sql}");
    }
}
