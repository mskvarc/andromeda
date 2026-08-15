//! Removing entities and attribute instances whose `expiresAt` has passed
//! (clause 4.22).

use crate::{
    entity::{mapper::stored_uri, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_entity::{entity_snapshot::EntitySnapshot, expiry::EntityExpiryBatchSize};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    jsonb::{jsonb_array_elements, jsonb_each, jsonb_typeof},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use chrono::{DateTime, Utc};
use sea_query::{Expr, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::Row;

/// The `expiresAt` member of an attribute instance, as the expanded JSON-LD
/// path a `#>>` extraction walks: the value node under the `expiresAt` term.
const EXPIRES_AT_VALUE_PATH: [&str; 3] = ["https://uri.etsi.org/ngsi-ld/expiresAt", "0", "@value"];

/// The derived table of an entity's array-valued attributes, aliased `kv`, whose
/// one column `val` is the JSONB array of one attribute's instances.
#[derive(Iden, Clone, Copy)]
#[iden = "kv"]
struct Kv;

/// The `jsonb_each` source the derived table reads its attributes from.
#[derive(Iden, Clone, Copy)]
#[iden = "attr"]
struct Attr;

/// The lateral expansion of one attribute's instance array, aliased `inst`.
#[derive(Iden, Clone, Copy)]
#[iden = "inst"]
struct Inst;

/// The `jsonb_array_elements` source the lateral expansion reads instances from.
#[derive(Iden, Clone, Copy)]
#[iden = "elem"]
struct Elem;

/// The `key` column `jsonb_each` projects.
#[derive(Iden, Clone, Copy)]
#[iden = "key"]
struct Key;

/// The `value` column `jsonb_each` and `jsonb_array_elements` project.
#[derive(Iden, Clone, Copy)]
#[iden = "value"]
struct ValueColumn;

/// The `val` column the derived table renames the attribute's array to.
#[derive(Iden, Clone, Copy)]
#[iden = "val"]
struct Val;

impl EntityRepository {
    /// Read identifiers of entities whose `expiresAt` has passed.
    pub async fn entities_due_for_expiry(&self, cutoff: DateTime<Utc>, batch_size: EntityExpiryBatchSize) -> Result<Vec<Uri>, ContextStoreError> {
        let (sql, values) = entities_due_for_expiry_statement(cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(String,)> = sqlx::query_as_with(&sql, values).fetch_all(&self.pool).await?;

        rows.into_iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Read every entity that holds a past-due attribute instance.
    pub async fn entities_with_expired_attributes(
        &self,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<(Uri, EntitySnapshot)>, ContextStoreError> {
        let (sql, values) = entities_with_expired_attributes_statement(cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;

        rows.into_iter()
            .map(|row| {
                Ok((
                    stored_uri(row.get("id"))?,
                    EntitySnapshot {
                        payload: row.get("payload"),
                        modified_at: row.get("modified_at"),
                    },
                ))
            })
            .collect()
    }
}

/// Build the entity-level expiry-candidate select.
///
/// A read only: the deletion an expiry implies is policy-owned and commits in a
/// transaction this statement is not in.
fn entities_due_for_expiry_statement(cutoff: DateTime<Utc>, limit: u64) -> SelectStatement {
    Query::select()
        .column(Entities::Id)
        .from(Entities::Table)
        .and_where(Expr::col(Entities::ExpiresAt).is_not_null())
        .and_where(Expr::col(Entities::ExpiresAt).lte(cutoff))
        .order_by(Entities::ExpiresAt, Order::Asc)
        .order_by(Entities::Id, Order::Asc)
        .limit(limit)
        .to_owned()
}

/// Build the select of every entity holding a past-due attribute instance,
/// returning the payload and the `modified_at` version token needed to rewrite
/// it under a precondition.
///
/// The instances live inside the payload, so the predicate walks it: the
/// entity's attributes are the array-valued members of `payload -> 0` that are
/// not JSON-LD keywords, each attribute's instances are the elements of that
/// array, and an instance is due when the `@value` under its `expiresAt` term is
/// at or before the cutoff. `jsonb_each` yields the attributes, a lateral
/// `jsonb_array_elements` yields each attribute's instances, and `#>>` extracts
/// the timestamp text a `::timestamptz` cast then compares.
fn entities_with_expired_attributes_statement(cutoff: DateTime<Utc>, limit: u64) -> SelectStatement {
    let attributes = attribute_arrays_of_entity();
    let instances = instances_of_attribute();

    let expiry_text = SimpleExpr::from(Expr::col((Inst, ValueColumn))).json_path_text(Expr::val(EXPIRES_AT_VALUE_PATH.map(str::to_owned).to_vec()).into());
    let expiry = cast_compound(expiry_text, PostgresType::Timestamptz);

    let mut existence = Query::select();
    existence
        .expr(Expr::val(1))
        .from_subquery(attributes, Kv)
        .join_lateral(JoinType::InnerJoin, instances, Inst, SimpleExpr::from(Expr::val(true)))
        .and_where(Expr::expr(expiry).lte(cutoff));
    let existence = existence;

    Query::select()
        .column(Entities::Id)
        .column(Entities::Payload)
        .column(Entities::ModifiedAt)
        .from(Entities::Table)
        .and_where(Expr::exists(existence))
        .order_by(Entities::ModifiedAt, Order::Asc)
        .order_by(Entities::Id, Order::Asc)
        .limit(limit)
        .to_owned()
}

/// The derived table `kv`: one row per array-valued, non-keyword attribute of
/// the enclosing entity, its `value` renamed `val`.
///
/// `payload -> 0` is the entity node; correlated to the outer `entities` row
/// rather than lateral, since it reads a column of the enclosing query.
fn attribute_arrays_of_entity() -> SelectStatement {
    let entity_node = SimpleExpr::from(Expr::col((Entities::Table, Entities::Payload))).json_get(Expr::val(0).into());
    Query::select()
        .expr_as(Expr::col((Attr, ValueColumn)), Val)
        .from_function(jsonb_each(entity_node), Attr)
        .and_where(Expr::col((Attr, Key)).not_like("@%"))
        .and_where(Expr::expr(jsonb_typeof(Expr::col((Attr, ValueColumn)).into())).eq("array"))
        .to_owned()
}

/// The lateral expansion `inst`: one row per instance of the attribute `kv.val`
/// holds, its `value` the instance node the predicate reads `expiresAt` from.
fn instances_of_attribute() -> SelectStatement {
    Query::select()
        .expr(Expr::col((Elem, ValueColumn)))
        .from_function(jsonb_array_elements(Expr::col((Kv, Val)).into()), Elem)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::entity::expiry::{entities_due_for_expiry_statement, entities_with_expired_attributes_statement};
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;
    use sea_query_binder::SqlxBinder;

    fn cutoff() -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")
    }

    #[test]
    fn the_entity_level_candidate_query_only_reads_due_rows_in_order() {
        let (sql, values) = entities_due_for_expiry_statement(cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "id" FROM "entities""#), "{sql}");
        assert!(sql.contains(r#""expires_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""expires_at" <= $1"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "expires_at" ASC, "id" ASC"#), "{sql}");
        assert!(!sql.contains("DELETE") && !sql.contains("UPDATE") && !sql.contains("NOW()"), "{sql}");
        assert_eq!(values.0.0.len(), 2, "the cutoff and the limit are bound: {sql}");
    }

    #[test]
    fn the_attribute_expiry_query_walks_the_payload_and_reads_the_version_token() {
        let (sql, _values) = entities_with_expired_attributes_statement(cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains(r#"SELECT "id", "payload", "modified_at" FROM "entities""#), "{sql}");
        assert!(sql.contains("jsonb_each"), "{sql}");
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
        assert!(sql.contains("jsonb_array_elements"), "{sql}");
        assert!(sql.contains(r#""inst"."value" #>>"#), "{sql}");
        assert!(sql.contains(")::timestamptz"), "{sql}");
    }

    #[test]
    fn the_attribute_expiry_query_excludes_keywords_and_scalars_and_writes_nothing() {
        let (sql, _values) = entities_with_expired_attributes_statement(cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains("NOT LIKE"), "keywords are skipped: {sql}");
        assert!(sql.contains("jsonb_typeof"), "only array-valued members are attributes: {sql}");
        for statement in ["UPDATE ", "DELETE ", "INSERT "] {
            assert!(!sql.contains(statement), "the candidate query must not mutate: found {statement} in {sql}");
        }
    }
}
