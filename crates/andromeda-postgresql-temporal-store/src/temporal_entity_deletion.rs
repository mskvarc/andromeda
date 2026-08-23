//! Temporal Evolution deletion (clause 5.6.16).
//!
//! Deleting the temporal evolution of an entity removes recorded history, the
//! streams that grouped it and the temporal entity itself. Nothing in the
//! database sequences that: the hypertables carry no foreign key back to the
//! catalogue, because a referential action on the broker's hottest write path
//! costs a lookup per inserted row, and no trigger cascades anything. The order
//! is therefore stated here, inside one transaction, and
//! [`audit_orphaned_temporal_rows`](TemporalRepository::audit_orphaned_temporal_rows)
//! is the check that the arrangement held.
//!
//! Core Entity deletion (clause 5.6.6) is a different operation and does not
//! reach this module: it leaves temporal state standing and records the
//! tombstones the specification requires instead.

use crate::{error::TemporalStoreError, repo::TemporalRepository, write_record_release::release_named};
use andromeda_context_space::{context_space_key::ContextSpaceKey, context_target::ContextTarget};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::aggregate::count_all;
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, JoinType, LockType, PostgresQueryBuilder, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The `attribute_history` alias in the orphan audit.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

/// The `temporal_attribute_streams` alias in the orphan audit.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `scope_history` alias in the orphan audit.
#[derive(Iden, Clone, Copy)]
#[iden = "sh"]
struct Sh;

/// The `temporal_entities` alias in the orphan audit.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
struct Te;

/// The count of attribute-history rows whose stream is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_attribute_history_rows"]
struct OrphanedAttributeHistoryRows;

/// The count of scope-history rows whose temporal entity is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_scope_history_rows"]
struct OrphanedScopeHistoryRows;

/// The count of streams whose temporal entity is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_streams"]
struct OrphanedStreams;

/// What one Temporal Evolution deletion removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalEvolutionDeletion {
    /// The temporal entity the deletion resolved and removed.
    pub entity_key: TemporalEntityKey,
    /// Attribute history rows removed.
    pub attribute_history_rows: u64,
    /// Scope history rows removed.
    pub scope_history_rows: u64,
    /// Attribute streams removed.
    pub streams: u64,
    /// Idempotency records released because they no longer protect an effect.
    pub released_write_records: u64,
}

/// Temporal rows whose owning row no longer exists.
///
/// Every count is expected to be zero. A non-zero count means a deletion did
/// not run its full sequence, which leaves history no query can reach and no
/// deletion can address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrphanedTemporalRows {
    /// Attribute history rows whose `stream_key` names no stream.
    pub attribute_history_rows: i64,
    /// Scope history rows whose `temporal_entity_key` names no temporal entity.
    pub scope_history_rows: i64,
    /// Streams whose `temporal_entity_key` names no temporal entity.
    pub streams: i64,
}

impl TemporalRepository {
    /// Delete the whole temporal evolution of one entity (clause 5.6.16).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NotFound`] when no temporal entity carries
    /// the identifier, [`TemporalStoreError::Database`] when a statement fails,
    /// and [`TemporalStoreError::Decode`] when the stored temporal-entity key is
    /// outside the range the database assigns.
    pub async fn delete_temporal_evolution(&self, target: &ContextTarget, entity_id: &Uri) -> Result<TemporalEvolutionDeletion, TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let entity_key = locate_temporal_entity(&mut tx, target.key(), entity_id).await?;
        let attribute_writes = delete_returning_write_ids(&mut tx, delete_attribute_history_statement(target.key(), entity_key)).await?;
        let scope_writes = delete_returning_write_ids(&mut tx, delete_scope_history_statement(target.key(), entity_key)).await?;
        let (streams_sql, streams_values) = delete_attribute_streams_statement(target.key(), entity_key).build_sqlx(PostgresQueryBuilder);
        let streams = sqlx::query_with(AssertSqlSafe(streams_sql), streams_values)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let (entity_sql, entity_values) = delete_temporal_entity_statement(target.key(), entity_key).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(AssertSqlSafe(entity_sql), entity_values).execute(&mut *tx).await?;
        let touched_writes = attribute_writes.iter().chain(scope_writes.iter()).copied().collect::<BTreeSet<Uuid>>();
        let touched_writes = touched_writes.into_iter().collect::<Vec<Uuid>>();
        let released_write_records = release_named(&mut tx, &touched_writes).await?;
        tx.commit().await?;
        Ok(TemporalEvolutionDeletion {
            entity_key,
            attribute_history_rows: attribute_writes.len() as u64,
            scope_history_rows: scope_writes.len() as u64,
            streams,
            released_write_records,
        })
    }

    /// Report temporal rows left behind by an incomplete deletion sequence.
    ///
    /// This is the audit that stands in for the foreign keys the hypertables do
    /// not carry. An empty report — every count zero — is the expected outcome.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the audit statement fails
    /// and [`TemporalStoreError::Decode`] when a returned count does not carry
    /// the expected type.
    pub async fn audit_orphaned_temporal_rows(&self) -> Result<OrphanedTemporalRows, TemporalStoreError> {
        let (sql, values) = audit_orphaned_statement().build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(OrphanedTemporalRows {
            attribute_history_rows: count(&row, "orphaned_attribute_history_rows")?,
            scope_history_rows: count(&row, "orphaned_scope_history_rows")?,
            streams: count(&row, "orphaned_streams")?,
        })
    }
}

/// Read one audit count, naming the column that failed to decode.
fn count(row: &PgRow, column: &str) -> Result<i64, TemporalStoreError> {
    row.try_get(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal orphan audit count {column}: {error}")))
}

/// Resolve and lock the temporal entity one deletion addresses.
async fn locate_temporal_entity(
    tx: &mut Transaction<'_, Postgres>,
    context_space_key: ContextSpaceKey,
    entity_id: &Uri,
) -> Result<TemporalEntityKey, TemporalStoreError> {
    let (sql, values) = lock_temporal_entity_statement(context_space_key, entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
    let located = sqlx::query_with(AssertSqlSafe(sql), values).fetch_optional(&mut **tx).await?;
    let Some(row) = located else {
        return Err(TemporalStoreError::NotFound(format!("Temporal evolution of {entity_id} not found")));
    };
    let stored: i64 = row
        .try_get("entity_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.entity_key: {error}")))?;
    TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))
}

/// Run one deletion statement, collecting the write identity of every removed row.
async fn delete_returning_write_ids(tx: &mut Transaction<'_, Postgres>, statement: DeleteStatement) -> Result<Vec<Uuid>, TemporalStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            row.try_get("write_id")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid history write_id: {error}")))
        })
        .collect()
}

/// Lock the addressed temporal entity before anything is removed.
fn lock_temporal_entity_statement(context_space_key: ContextSpaceKey, id: &str) -> SelectStatement {
    Query::select()
        .column(TemporalEntities::EntityKey)
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::ContextSpaceKey).eq(context_space_key.as_i64()))
        .and_where(Expr::col(TemporalEntities::Id).eq(id))
        .lock(LockType::Update)
        .to_owned()
}

/// Remove one entity's attribute history, reporting each removed write identity.
fn delete_attribute_history_statement(context_space_key: ContextSpaceKey, entity_key: TemporalEntityKey) -> DeleteStatement {
    let owned_streams = Query::select()
        .column((S, TemporalAttributeStreams::StreamKey))
        .from_as(TemporalAttributeStreams::Table, S)
        .and_where(Expr::col((S, TemporalAttributeStreams::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).eq(entity_key.get()))
        .to_owned();
    Query::delete()
        .from_table(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::ContextSpaceKey).eq(context_space_key.as_i64()))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(owned_streams))
        .returning(Query::returning().column(AttributeHistory::WriteId))
        .to_owned()
}

/// Remove one entity's scope history, reporting each removed write identity.
fn delete_scope_history_statement(context_space_key: ContextSpaceKey, entity_key: TemporalEntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(ScopeHistory::Table)
        .and_where(Expr::col(ScopeHistory::ContextSpaceKey).eq(context_space_key.as_i64()))
        .and_where(Expr::col(ScopeHistory::TemporalEntityKey).eq(entity_key.get()))
        .returning(Query::returning().column(ScopeHistory::WriteId))
        .to_owned()
}

/// Remove one entity's attribute streams.
///
/// The stream's foreign key cascades from `temporal_entities`, so this is not
/// what keeps the catalogue consistent; it is stated anyway to keep the sequence
/// explicit and to report what the deletion removed.
fn delete_attribute_streams_statement(context_space_key: ContextSpaceKey, entity_key: TemporalEntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAttributeStreams::Table)
        .and_where(Expr::col(TemporalAttributeStreams::ContextSpaceKey).eq(context_space_key.as_i64()))
        .and_where(Expr::col(TemporalAttributeStreams::TemporalEntityKey).eq(entity_key.get()))
        .to_owned()
}

/// Remove the temporal entity itself.
fn delete_temporal_entity_statement(context_space_key: ContextSpaceKey, entity_key: TemporalEntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::ContextSpaceKey).eq(context_space_key.as_i64()))
        .and_where(Expr::col(TemporalEntities::EntityKey).eq(entity_key.get()))
        .to_owned()
}

/// The orphan-count subquery of one dangling reference: rows of `child` whose
/// `parent` no longer exists, via a left join that keeps the unmatched rows.
fn orphan_count(child: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(child.into_sub_query_statement()))
}

/// The audit that stands in for the foreign keys the hypertables do not carry:
/// one orphan count per reference no constraint enforces.
fn audit_orphaned_statement() -> SelectStatement {
    let orphaned_history = Query::select()
        .expr(count_all())
        .from_as(AttributeHistory::Table, Ah)
        .join_as(
            JoinType::LeftJoin,
            TemporalAttributeStreams::Table,
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((Ah, AttributeHistory::StreamKey)),
        )
        .and_where(Expr::col((S, TemporalAttributeStreams::StreamKey)).is_null())
        .to_owned();
    let orphaned_scope = Query::select()
        .expr(count_all())
        .from_as(ScopeHistory::Table, Sh)
        .join_as(
            JoinType::LeftJoin,
            TemporalEntities::Table,
            Te,
            Expr::col((Te, TemporalEntities::EntityKey)).equals((Sh, ScopeHistory::TemporalEntityKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::EntityKey)).is_null())
        .to_owned();
    let orphaned_streams = Query::select()
        .expr(count_all())
        .from_as(TemporalAttributeStreams::Table, S)
        .join_as(
            JoinType::LeftJoin,
            TemporalEntities::Table,
            Te,
            Expr::col((Te, TemporalEntities::EntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::EntityKey)).is_null())
        .to_owned();
    Query::select()
        .expr_as(orphan_count(orphaned_history), OrphanedAttributeHistoryRows)
        .expr_as(orphan_count(orphaned_scope), OrphanedScopeHistoryRows)
        .expr_as(orphan_count(orphaned_streams), OrphanedStreams)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::temporal_entity_deletion::{
        audit_orphaned_statement,
        delete_attribute_history_statement,
        delete_attribute_streams_statement,
        delete_scope_history_statement,
        delete_temporal_entity_statement,
        lock_temporal_entity_statement,
    };
    use andromeda_context_space::context_space_key::ContextSpaceKey;
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use sea_query::PostgresQueryBuilder;

    fn key() -> TemporalEntityKey {
        TemporalEntityKey::try_from(1).expect("a positive key")
    }

    fn context_space_key() -> ContextSpaceKey {
        ContextSpaceKey::try_from(1).expect("positive context-space key")
    }

    #[test]
    fn the_addressed_temporal_entity_is_locked_before_anything_is_removed() {
        let sql = lock_temporal_entity_statement(context_space_key(), "urn:ngsi-ld:Vehicle:1").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "temporal_entities" WHERE "id" ="#), "{sql}");
        assert!(sql.contains("FOR UPDATE"), "{sql}");
    }

    #[test]
    fn history_removal_reports_the_write_identity_of_every_removed_row() {
        assert!(
            delete_attribute_history_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .contains(r#"RETURNING "write_id""#)
        );
        assert!(
            delete_scope_history_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .contains(r#"RETURNING "write_id""#)
        );
    }

    #[test]
    fn the_sequence_reaches_every_relation_the_evolution_occupies() {
        assert!(
            delete_attribute_history_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "attribute_history""#)
        );
        assert!(
            delete_scope_history_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "scope_history""#)
        );
        assert!(
            delete_attribute_streams_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "temporal_attribute_streams""#)
        );
        assert!(
            delete_temporal_entity_statement(context_space_key(), key())
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "temporal_entities""#)
        );
    }

    #[test]
    fn the_audit_covers_both_references_no_foreign_key_enforces() {
        let sql = audit_orphaned_statement().to_string(PostgresQueryBuilder);
        for column in ["orphaned_attribute_history_rows", "orphaned_scope_history_rows", "orphaned_streams"] {
            assert!(sql.contains(column), "missing {column}: {sql}");
        }
        assert_eq!(sql.matches("COUNT(*)").count(), 3, "{sql}");
    }

    #[test]
    fn no_statement_records_a_tombstone_for_a_temporal_evolution_deletion() {
        for sql in [
            delete_attribute_history_statement(context_space_key(), key()).to_string(PostgresQueryBuilder),
            delete_scope_history_statement(context_space_key(), key()).to_string(PostgresQueryBuilder),
            delete_attribute_streams_statement(context_space_key(), key()).to_string(PostgresQueryBuilder),
            delete_temporal_entity_statement(context_space_key(), key()).to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains("INSERT"), "{sql}");
            assert!(!sql.contains("deleted_at"), "{sql}");
        }
    }
}
