//! Removing entities and attribute instances whose `expiresAt` has passed
//! (clause 4.22).

use crate::{
    entity::{mapper::stored_uri, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, expiry::EntityExpiryBatchSize};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{E, Entities, EntityDocuments};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::Row;

/// Alias `d` for the `entity_documents` row the attribute-expiry candidate
/// read joins to the identity row.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

impl EntityRepository {
    /// Read identifiers of entities whose `expiresAt` has passed.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the candidate query fails, or when a
    /// returned identifier is no longer a valid URI.
    pub async fn entities_due_for_expiry(
        &self,
        target: &ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<Uri>, ContextStoreError> {
        let (sql, values) = entities_due_for_expiry_statement(target, cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        rows.iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Read every entity that holds a past-due attribute instance.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the candidate query fails, or when a
    /// returned identifier is no longer a valid URI.
    pub async fn entities_with_expired_attributes(
        &self,
        target: &ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<(Uri, EntitySnapshot)>, ContextStoreError> {
        let (sql, values) = entities_with_expired_attributes_statement(target, cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

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
fn entities_due_for_expiry_statement(target: &ContextTarget, cutoff: DateTime<Utc>, limit: u64) -> SelectStatement {
    Query::select()
        .column(Entities::Id)
        .from(table_of(target, Entities::Table))
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
/// ```sql
/// SELECT e.id, d.payload, d.modified_at FROM entities e
/// JOIN entity_documents d ON d.entity_key = e.entity_key
/// WHERE e.min_attribute_expires_at IS NOT NULL AND e.min_attribute_expires_at <= $1
/// ORDER BY e.min_attribute_expires_at, e.id LIMIT $2
/// ```
///
/// The write path records the earliest instance expiry of every payload on the
/// identity row, so the candidates are a range scan of the partial index over
/// that column rather than a walk of every payload. The sweep re-reads each
/// candidate's payload and decides instance by instance; the column is only
/// the coarse filter, and a candidate whose due instance was already removed by
/// a concurrent write simply yields no purge.
fn entities_with_expired_attributes_statement(target: &ContextTarget, cutoff: DateTime<Utc>, limit: u64) -> SelectStatement {
    Query::select()
        .expr(Expr::col((E, Entities::Id)))
        .expr(Expr::col((D, EntityDocuments::Payload)))
        .expr(Expr::col((D, EntityDocuments::ModifiedAt)))
        .from(table_of(target, Entities::Table).alias(E))
        .join_as(
            JoinType::Join,
            table_of(target, EntityDocuments::Table),
            D,
            Expr::col((D, EntityDocuments::EntityKey)).equals((E, Entities::EntityKey)),
        )
        .and_where(Expr::col((E, Entities::MinAttributeExpiresAt)).is_not_null())
        .and_where(Expr::col((E, Entities::MinAttributeExpiresAt)).lte(cutoff))
        .order_by((E, Entities::MinAttributeExpiresAt), Order::Asc)
        .order_by((E, Entities::Id), Order::Asc)
        .limit(limit)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::entity::expiry::{entities_due_for_expiry_statement, entities_with_expired_attributes_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;

    fn cutoff() -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")
    }

    #[test]
    fn the_entity_level_candidate_query_only_reads_due_rows_in_order() {
        let (sql, values) = entities_due_for_expiry_statement(&ContextTarget::Live, cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "id" FROM "entities""#), "{sql}");
        assert!(sql.contains(r#""expires_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""expires_at" <= $1"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "expires_at" ASC, "id" ASC"#), "{sql}");
        assert!(!sql.contains("DELETE") && !sql.contains("UPDATE") && !sql.contains("NOW()"), "{sql}");
        assert_eq!(values.0.0.len(), 2, "the cutoff and limit are bound: {sql}");
    }

    #[test]
    fn the_attribute_expiry_query_ranges_over_the_recorded_minimum_and_reads_the_version_token() {
        let (sql, values) = entities_with_expired_attributes_statement(&ContextTarget::Live, cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains(r#""e"."min_attribute_expires_at" <= $1"#), "{sql}");
        assert_eq!(values.0.0.len(), 2, "the cutoff and limit are bound: {sql}");
    }

    #[test]
    fn the_attribute_expiry_query_walks_no_payload_and_writes_nothing() {
        let (sql, _values) = entities_with_expired_attributes_statement(&ContextTarget::Live, cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        for fragment in ["jsonb_each", "jsonb_array_elements", "#>>", "::timestamptz", "UPDATE ", "DELETE ", "INSERT "] {
            assert!(!sql.contains(fragment), "the candidate query must not contain {fragment}: {sql}");
        }
    }
}
