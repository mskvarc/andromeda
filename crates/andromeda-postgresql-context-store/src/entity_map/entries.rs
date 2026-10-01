//! The statements over a map's `entity_map_entries` and `entity_map_links`.
//!
//! Both are written in bulk through `UNNEST`, so a map over thousands of
//! entities is one statement per child table rather than one per row.

use crate::entity_map::{
    lifecycle::{entity_map_key_of, live_entity_map_key_of},
    persistence_error,
};
use andromeda_entity_map::{error::EntityMapError, store::EntityMapMutation};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_postgresql_schema::entity_map::{EntityMapEntries, EntityMapLinks, EntityMaps as EntityMapsTable};
use andromeda_postgresql_seaquery::{
    clock::now,
    pg_array::{any, text_array},
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, IntoIden, OnConflict, Order, PostgresQueryBuilder, Query};
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
        .expr(entity_map_key_of(external_placeholder(1)))
        .expr(Expr::col((D, EntityMapEntries::Position)))
        .expr(Expr::col((D, EntityMapEntries::EntityId)))
        .expr(Expr::col((D, EntityMapEntries::Source)))
        .from_function(call, D);
    let mut insert = Query::insert();
    insert.into_table(EntityMapEntries::Table).columns([
        EntityMapEntries::EntityMapKey.into_iden(),
        EntityMapEntries::Position.into_iden(),
        EntityMapEntries::EntityId.into_iden(),
        EntityMapEntries::Source.into_iden(),
    ]);
    insert.select_from(select).map_err(|error| persistence_error(error))?;
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
        .expr(entity_map_key_of(external_placeholder(1)))
        .expr(Expr::col((D, EntityMapLinks::RegistrationId)))
        .expr(Expr::col((D, EntityMapLinks::LinkedMapId)))
        .from_function(call, D);
    let mut insert = Query::insert();
    insert.into_table(EntityMapLinks::Table).columns([
        EntityMapLinks::EntityMapKey.into_iden(),
        EntityMapLinks::RegistrationId.into_iden(),
        EntityMapLinks::LinkedMapId.into_iden(),
    ]);
    insert.select_from(select).map_err(|error| persistence_error(error))?;
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
                position: i32::try_from(position).map_err(|error| persistence_error(error))?,
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
            .map_err(|error| persistence_error(error))?;
    }
    if !map.linked_maps.is_empty() {
        let registrations: Vec<&str> = map.linked_maps.keys().map(Identifier::as_str).collect();
        let linked: Vec<&str> = map.linked_maps.values().map(Identifier::as_str).collect();
        let sql = links_insert_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(map.id.as_str())
            .bind(&registrations)
            .bind(&linked)
            .execute(&mut *connection)
            .await
            .map_err(|error| persistence_error(error))?;
    }
    Ok(())
}

/// The upsert of one link under a live map: the row is selected off the live
/// control record, so an absent or expired map writes nothing, and a link
/// already recorded for the registration is replaced.
fn link_upsert(id: &Identifier, registration: &Identifier, linked_map: &Identifier) -> Result<InsertStatement, EntityMapError> {
    let mut staged = Query::select();
    staged
        .column((EntityMapsTable::Table, EntityMapsTable::EntityMapKey))
        .expr(Expr::val(registration.as_str()))
        .expr(Expr::val(linked_map.as_str()))
        .from(EntityMapsTable::Table)
        .and_where(Expr::col((EntityMapsTable::Table, EntityMapsTable::Id)).eq(id.as_str()))
        .and_where(Expr::col((EntityMapsTable::Table, EntityMapsTable::ExpiresAt)).gt(now()));
    let mut insert = Query::insert();
    insert
        .into_table(EntityMapLinks::Table)
        .columns([
            EntityMapLinks::EntityMapKey.into_iden(),
            EntityMapLinks::RegistrationId.into_iden(),
            EntityMapLinks::LinkedMapId.into_iden(),
        ])
        .on_conflict(
            OnConflict::columns([EntityMapLinks::EntityMapKey, EntityMapLinks::RegistrationId])
                .update_column(EntityMapLinks::LinkedMapId)
                .to_owned(),
        );
    insert.select_from(staged).map_err(|error| persistence_error(error))?;
    Ok(insert)
}

/// Record the map a registration's context source used under a live map,
/// replacing the link recorded for that registration (clause 5.5.14).
pub(crate) async fn upsert_link(
    pool: &PgPool,
    id: &Identifier,
    registration: &Identifier,
    linked_map: &Identifier,
) -> Result<EntityMapMutation, EntityMapError> {
    let (sql, values) = link_upsert(id, registration, linked_map)?.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(match result.rows_affected() {
        0 => EntityMapMutation::Missing,
        1.. => EntityMapMutation::Applied,
    })
}

/// The delete shedding entities from a live map: every `(entity, source)` row
/// naming one of `entity_ids` goes, so the entity leaves the map together with
/// the registrations it was attributed to.
///
/// The map key is resolved off the live control record, so an expired map
/// resolves to NULL and nothing is removed.
fn entries_delete(id: &Identifier, entity_ids: &[&str]) -> DeleteStatement {
    let mut delete = Query::delete();
    delete
        .from_table(EntityMapEntries::Table)
        .and_where(Expr::col(EntityMapEntries::EntityMapKey).eq(live_entity_map_key_of(Expr::val(id.as_str()))))
        .and_where(Expr::col(EntityMapEntries::EntityId).eq(any(text_array(entity_ids))));
    delete
}

/// Remove `entity_ids` from a live map (clauses 4.3.6.7 and 5.5.14): the
/// identifiers a prepared result page checked and found no longer matching
/// stop being considered by the pages that follow.
pub(crate) async fn prune_entries(pool: &PgPool, id: &Identifier, entity_ids: &[Identifier]) -> Result<(), EntityMapError> {
    let entity_ids: Vec<&str> = entity_ids.iter().map(Identifier::as_str).collect();
    let (sql, values) = entries_delete(id, &entity_ids).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(())
}

/// A map's `(entity_id, source)` pairs in the order they were produced, which
/// is the order of the table's key, so the read needs no sort.
pub(crate) async fn read_entries(pool: &PgPool, id: &Identifier) -> Result<Vec<(String, String)>, EntityMapError> {
    let (sql, values) = Query::select()
        .column(EntityMapEntries::EntityId)
        .column(EntityMapEntries::Source)
        .from(EntityMapEntries::Table)
        .and_where(Expr::col(EntityMapEntries::EntityMapKey).eq(entity_map_key_of(Expr::val(id.as_str()))))
        .order_by(EntityMapEntries::Position, Order::Asc)
        .build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(|error| persistence_error(error))?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("entity_id").map_err(|error| persistence_error(error))?,
                row.try_get("source").map_err(|error| persistence_error(error))?,
            ))
        })
        .collect()
}

/// A map's `(registration_id, linked_map_id)` pairs.
pub(crate) async fn read_links(pool: &PgPool, id: &Identifier) -> Result<Vec<(String, String)>, EntityMapError> {
    let (sql, values) = Query::select()
        .column(EntityMapLinks::RegistrationId)
        .column(EntityMapLinks::LinkedMapId)
        .from(EntityMapLinks::Table)
        .and_where(Expr::col(EntityMapLinks::EntityMapKey).eq(entity_map_key_of(Expr::val(id.as_str()))))
        .order_by(EntityMapLinks::RegistrationId, Order::Asc)
        .build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(|error| persistence_error(error))?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("registration_id").map_err(|error| persistence_error(error))?,
                row.try_get("linked_map_id").map_err(|error| persistence_error(error))?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::entity_map::entries::{entries_delete, entries_insert_sql, link_upsert, links_insert_sql};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_entries_insert_stages_three_arrays_under_the_resolved_entity_map_key() {
        let sql = entries_insert_sql().expect("builds");
        assert!(
            sql.contains(r#"INSERT INTO "entity_map_entries" ("entity_map_key", "position", "entity_id", "source")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "entity_maps"."entity_map_key" FROM "entity_maps" WHERE "entity_maps"."id" = ($1))"#),
            "{sql}"
        );
        assert!(
            sql.contains("UNNEST($2::integer[], $3::text[], $4::text[]) AS d(position, entity_id, source)"),
            "{sql}"
        );
    }

    #[test]
    fn the_links_insert_stages_two_arrays_under_the_resolved_entity_map_key() {
        let sql = links_insert_sql().expect("builds");
        assert!(
            sql.contains(r#"INSERT INTO "entity_map_links" ("entity_map_key", "registration_id", "linked_map_id")"#),
            "{sql}"
        );
        assert!(sql.contains("UNNEST($2::text[], $3::text[]) AS d(registration_id, linked_map_id)"), "{sql}");
    }

    /// Clause 4.3.6.7: the identifiers shed by a page preparation leave the map
    /// with every source attributed to them, and only while the map is live.
    #[test]
    fn the_prune_delete_removes_the_named_entities_from_the_live_map_alone() {
        let (sql, values) = entries_delete(
            &"urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
            &["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"],
        )
        .build(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "entity_map_entries""#), "{sql}");
        assert!(
            sql.contains(r#""entity_map_key" = (SELECT "entity_maps"."entity_map_key" FROM "entity_maps" WHERE "entity_maps"."id" = $1 AND "entity_maps"."expires_at" > "#),
            "{sql}"
        );
        assert!(sql.contains(r#""entity_id" = ANY($2)"#), "{sql}");
        assert_eq!(values.0.len(), 2, "the map URI and the whole identifier array are the only bound values: {sql}");
    }

    #[test]
    fn the_link_upsert_selects_the_live_map_key_and_replaces_the_registration_link() {
        let sql = link_upsert(
            &"urn:ngsi-ld:EntityMap:1".parse().expect("valid URI"),
            &"urn:ngsi-ld:csr:b".parse().expect("valid URI"),
            &"urn:ngsi-ld:EntityMap:b".parse().expect("valid URI"),
        )
        .expect("builds")
        .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"INSERT INTO "entity_map_links" ("entity_map_key", "registration_id", "linked_map_id")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""entity_maps"."id" = 'urn:ngsi-ld:EntityMap:1'"#), "{sql}");
        assert!(sql.contains(r#""entity_maps"."expires_at" > "#), "{sql}");
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_map_key", "registration_id") DO UPDATE SET "linked_map_id" = "excluded"."linked_map_id""#),
            "{sql}"
        );
    }
}
