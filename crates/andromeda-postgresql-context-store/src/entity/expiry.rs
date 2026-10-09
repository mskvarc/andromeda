//! Removing entities and attribute instances whose `expiresAt` has passed
//! (clause 4.22).

use crate::{
    current_attribute::read::RowSelection,
    entity::{mapper::stored_uri, repo::EntityRepository, stored_entities::heads_statement},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, expiry::EntityExpiryBatchSize};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{E, Entities};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;

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
    ) -> Result<Vec<Identifier>, ContextStoreError> {
        let (sql, values) = entities_due_for_expiry_statement(target, cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(String,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        rows.iter().map(|(id,)| stored_uri(id)).collect()
    }

    /// Read every entity that holds a past-due attribute instance.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a candidate read or a name lookup
    /// fails, or when the stored rows no longer read back as entity states.
    pub async fn entities_with_expired_attributes(
        &self,
        target: &ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<(Identifier, EntitySnapshot)>, ContextStoreError> {
        let (sql, values) = entities_with_expired_attributes_statement(target, cutoff, u64::from(batch_size.get())).build_sqlx(PostgresQueryBuilder);
        let heads = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        Ok(self
            .entities_of_heads(target, &heads, &RowSelection::Every)
            .await?
            .into_iter()
            .map(|retrieved| {
                // clone: the identifier keys the candidate and stays in the state it names
                let id = retrieved.state.id().clone();
                (
                    id,
                    EntitySnapshot {
                        state: retrieved.state,
                        modified_at: retrieved.sys_attrs.modified_at,
                    },
                )
            })
            .collect())
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

/// Build the select of the heads of every entity holding a past-due attribute
/// instance, whose `modified_at` is the version token needed to rewrite it
/// under a precondition; its rows are read afterwards by key.
///
/// ```sql
/// SELECT <head> FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key
/// WHERE e.min_attribute_expires_at IS NOT NULL AND e.min_attribute_expires_at <= $1
/// ORDER BY e.min_attribute_expires_at, e.id LIMIT $2
/// ```
///
/// The write path records the earliest instance expiry of every entity on the
/// identity row, so the candidates are a range scan of the partial index over
/// that column rather than a walk of every row. The sweep reads each
/// candidate's rows and decides instance by instance; the column is only the
/// coarse filter, and a candidate whose due instance was already removed by a
/// concurrent write simply yields no purge.
fn entities_with_expired_attributes_statement(target: &ContextTarget, cutoff: DateTime<Utc>, limit: u64) -> SelectStatement {
    heads_statement(target)
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
    use pretty_assertions::assert_eq;
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
    fn the_attribute_expiry_query_reads_heads_and_writes_nothing() {
        let (sql, _values) = entities_with_expired_attributes_statement(&ContextTarget::Live, cutoff(), 100).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains(r#""e"."modified_at" AS "entity_modified_at""#), "{sql}");
        for fragment in ["entity_documents", "current_attributes", "UPDATE ", "DELETE ", "INSERT "] {
            assert!(!sql.contains(fragment), "the candidate query must not contain {fragment}: {sql}");
        }
    }
}
