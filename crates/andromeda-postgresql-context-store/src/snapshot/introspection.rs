//! The summaries read over a snapshot's captured entities: the distinct types
//! and attribute names it holds, and the per-type and per-attribute details
//! the snapshot type and attribute discovery answer with.
//!
//! Every statement scopes `snapshot_entities` by the snapshot's surrogate,
//! resolved from the URI it is handed through
//! [`andromeda_postgresql_context_query::snapshot_key::snapshot_key_of`].

use crate::snapshot::persistence_error;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_context_query::snapshot_key::snapshot_key_of;
use andromeda_postgresql_schema::snapshot::SnapshotEntities;
use andromeda_postgresql_seaquery::{aggregate::count_all, jsonb::jsonb_object_keys, pg_operator::PgBinaryOperator};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{SnapshotAttributeInfo, SnapshotTypeInfo},
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgPool, Row};

/// The `t` output column of a distinct-type read.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct TypeColumn;

/// The `cnt` output column of a count read.
#[derive(Iden, Clone, Copy)]
#[iden = "cnt"]
struct CountColumn;

/// The `key` set-returning source and its single output column.
#[derive(Iden, Clone, Copy)]
#[iden = "key"]
struct KeyColumn;

/// The keyword members `jsonb_object_keys` yields that are not attributes.
const NON_ATTRIBUTE_KEYS: [&str; 3] = ["id", "type", "@context"];

/// `entity_data ->> 'type'` - the entity's type token as text.
fn entity_type_text(table: impl IntoIden) -> SimpleExpr {
    SimpleExpr::from(Expr::col((table.into_iden(), SnapshotEntities::EntityData))).json_get_text(Expr::val("type"))
}

/// The `snapshot_key = (SELECT … WHERE id = <snapshot_id>)` scope every summary
/// narrows to.
fn scoped_to(snapshot_id: &Uri) -> SimpleExpr {
    Expr::col(SnapshotEntities::SnapshotKey).eq(snapshot_key_of(Expr::val(snapshot_id.as_str())))
}

/// `SELECT DISTINCT entity_data ->> 'type' AS t FROM snapshot_entities WHERE … ORDER BY t`,
/// before the caller adds its scope.
fn distinct_types() -> SelectStatement {
    Query::select()
        .distinct()
        .expr_as(entity_type_text(SnapshotEntities::Table), TypeColumn)
        .from(SnapshotEntities::Table)
        .and_where(SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val("type")))
        .order_by(TypeColumn, Order::Asc)
        .to_owned()
}

/// The `SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key WHERE key NOT IN (…) ORDER BY key`
/// base a distinct-attribute read shares, before the caller adds its scope.
pub(crate) fn distinct_attribute_keys() -> SelectStatement {
    Query::select()
        .distinct()
        .column(KeyColumn)
        .from(SnapshotEntities::Table)
        .from_function(jsonb_object_keys(Expr::col((SnapshotEntities::Table, SnapshotEntities::EntityData))), KeyColumn)
        .and_where(Expr::col(KeyColumn).is_not_in(NON_ATTRIBUTE_KEYS))
        .order_by(KeyColumn, Order::Asc)
        .to_owned()
}

/// `SELECT COUNT(*) AS cnt FROM snapshot_entities`, before the caller adds its
/// scope.
fn count_rows() -> SelectStatement {
    Query::select().expr_as(count_all(), CountColumn).from(SnapshotEntities::Table).to_owned()
}

/// Parse one entity type decoded from a snapshot projection.
fn parse_snapshot_entity_type(raw: String) -> Result<EntityTypeName, SnapshotError> {
    raw.parse::<EntityTypeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot entity type '{raw}' in database: {error}")))
}

/// Parse one attribute name decoded from a snapshot projection.
fn parse_snapshot_attribute_name(raw: String) -> Result<AttributeName, SnapshotError> {
    raw.parse::<AttributeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot attribute name '{raw}' in database: {error}")))
}

/// Run a distinct-type statement and parse its `t` column.
async fn fetch_types(pool: &PgPool, statement: &SelectStatement) -> Result<Vec<EntityTypeName>, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let mut types = Vec::with_capacity(rows.len());
    for row in &rows {
        if let Some(raw) = row.try_get::<Option<String>, _>("t").map_err(persistence_error)? {
            types.push(parse_snapshot_entity_type(raw)?);
        }
    }
    Ok(types)
}

/// Run a distinct-attribute statement and parse its `key` column.
async fn fetch_attribute_names(pool: &PgPool, statement: &SelectStatement) -> Result<Vec<AttributeName>, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let mut attrs = Vec::with_capacity(rows.len());
    for row in &rows {
        let raw: String = row.try_get("key").map_err(persistence_error)?;
        attrs.push(parse_snapshot_attribute_name(raw)?);
    }
    Ok(attrs)
}

/// Run a count statement and read its `cnt` column.
async fn fetch_count(pool: &PgPool, statement: &SelectStatement) -> Result<i64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    row.try_get("cnt").map_err(persistence_error)
}

/// The distinct entity types a snapshot captured, in byte order.
pub(crate) async fn snapshot_distinct_types(pool: &PgPool, snapshot_id: &Uri) -> Result<Vec<EntityTypeName>, SnapshotError> {
    let mut statement = distinct_types();
    statement.and_where(scoped_to(snapshot_id));
    fetch_types(pool, &statement).await
}

/// How many captured entities carry `type_name`, and which attribute names
/// they declare between them; `None` when the snapshot holds no such entity.
pub(crate) async fn snapshot_type_info(pool: &PgPool, snapshot_id: &Uri, type_name: &EntityTypeName) -> Result<Option<SnapshotTypeInfo>, SnapshotError> {
    let mut count = count_rows();
    count
        .and_where(scoped_to(snapshot_id))
        .and_where(Expr::expr(entity_type_text(SnapshotEntities::Table)).eq(type_name.as_str()));
    let count = fetch_count(pool, &count).await?;
    if count == 0 {
        return Ok(None);
    }

    let mut attributes = distinct_attribute_keys();
    attributes
        .and_where(scoped_to(snapshot_id))
        .and_where(Expr::expr(entity_type_text(SnapshotEntities::Table)).eq(type_name.as_str()));
    let attrs = fetch_attribute_names(pool, &attributes).await?;
    Ok(Some((count, attrs)))
}

/// The distinct attribute names a snapshot's captured entities declare.
pub(crate) async fn snapshot_distinct_attributes(pool: &PgPool, snapshot_id: &Uri) -> Result<Vec<AttributeName>, SnapshotError> {
    let mut attributes = distinct_attribute_keys();
    attributes.and_where(scoped_to(snapshot_id));
    fetch_attribute_names(pool, &attributes).await
}

/// How many captured entities declare `attr_name`, and which types they carry
/// between them; `None` when no captured entity declares it.
pub(crate) async fn snapshot_attribute_info(
    pool: &PgPool,
    snapshot_id: &Uri,
    attr_name: &AttributeName,
) -> Result<Option<SnapshotAttributeInfo>, SnapshotError> {
    let declares = || SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val(attr_name.as_str()));

    let mut count = count_rows();
    count.and_where(scoped_to(snapshot_id)).and_where(declares());
    let count = fetch_count(pool, &count).await?;
    if count == 0 {
        return Ok(None);
    }

    let mut types = distinct_types();
    types.and_where(scoped_to(snapshot_id)).and_where(declares());
    let types = fetch_types(pool, &types).await?;
    Ok(Some((count, types)))
}

#[cfg(test)]
mod tests {
    use crate::snapshot::introspection::{distinct_attribute_keys, distinct_types, scoped_to};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn distinct_attributes_skip_the_keyword_members_and_walk_the_object_keys() {
        let sql = distinct_attribute_keys().to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_object_keys"), "{sql}");
        assert!(sql.contains(r#""key" NOT IN ('id', 'type', '@context')"#), "{sql}");
        assert!(sql.contains(r#"SELECT DISTINCT "key""#), "{sql}");
    }

    #[test]
    fn every_summary_scopes_the_child_table_by_the_resolved_surrogate() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let mut statement = distinct_types();
        statement.and_where(scoped_to(&snapshot_id));
        let sql = statement.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""snapshot_key" = (SELECT "snapshots"."snapshot_key" FROM "snapshots" WHERE "snapshots"."id" = 'urn:ngsi-ld:Snapshot:1')"#),
            "{sql}"
        );
        assert!(!sql.contains("snapshot_id"), "the child table carries no identifier column: {sql}");
    }
}
