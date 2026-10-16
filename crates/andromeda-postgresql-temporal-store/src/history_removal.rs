//! Removing every history row of whole streams, or of one temporal entity's
//! Scope, together with the write identities those rows carried.
//!
//! On a converted chunk TimescaleDB removes a compressed batch without
//! decompressing it only when every predicate binds the segmenting column to a
//! value and the statement returns nothing; anything else decompresses each
//! batch it reaches, row by row, under a per-transaction limit that a long
//! stream exceeds. So the segment is bound as values here, never as a
//! subquery, and the write identities are read before the rows go rather than
//! returned by the delete.

use crate::error::TemporalStoreError;
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::any};
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// What removing the history of one segment did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RemovedHistory {
    /// History rows removed.
    pub(crate) rows: u64,
    /// The distinct write identities the removed rows carried.
    pub(crate) writes: Vec<Uuid>,
}

/// A bound `bigint[]` value from a batch of stream keys.
///
/// The array value owns its elements, so the slice is copied: a bound parameter
/// cannot borrow from the caller's slice across the round trip.
pub(crate) fn stream_key_array(keys: &[i64]) -> SimpleExpr {
    Expr::val(keys.to_vec())
}

/// Remove every attribute history row of `stream_keys`.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a statement fails.
pub(crate) async fn remove_stream_history(
    tx: &mut Transaction<'_, Postgres>,
    schema: Option<&DynIden>,
    stream_keys: &[i64],
) -> Result<RemovedHistory, TemporalStoreError> {
    remove(tx, stream_writes_query(schema, stream_keys), stream_history_delete(schema, stream_keys)).await
}

/// Remove every Scope history row of the temporal entity `entity_key`.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a statement fails.
pub(crate) async fn remove_scope_history(
    tx: &mut Transaction<'_, Postgres>,
    schema: Option<&DynIden>,
    entity_key: TemporalEntityKey,
) -> Result<RemovedHistory, TemporalStoreError> {
    remove(tx, scope_writes_query(schema, entity_key), scope_history_delete(schema, entity_key)).await
}

/// Read the write identities `writes` selects, then run `delete`.
async fn remove(tx: &mut Transaction<'_, Postgres>, writes: SelectStatement, delete: DeleteStatement) -> Result<RemovedHistory, TemporalStoreError> {
    let (writes_sql, writes_values) = writes.build_sqlx(PostgresQueryBuilder);
    let writes: Vec<Uuid> = sqlx::query_scalar_with(AssertSqlSafe(writes_sql), writes_values).fetch_all(&mut **tx).await?;
    let (delete_sql, delete_values) = delete.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(AssertSqlSafe(delete_sql), delete_values)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(RemovedHistory { rows, writes })
}

/// The distinct write identities of the history rows of `stream_keys`.
fn stream_writes_query(schema: Option<&DynIden>, stream_keys: &[i64]) -> SelectStatement {
    Query::select()
        .distinct()
        .column(AttributeHistory::WriteId)
        .from(qualified(schema, AttributeHistory::Table))
        .and_where(Expr::col(AttributeHistory::StreamKey).eq(any(stream_key_array(stream_keys))))
        .to_owned()
}

/// Remove the history rows of `stream_keys`.
fn stream_history_delete(schema: Option<&DynIden>, stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, AttributeHistory::Table))
        .and_where(Expr::col(AttributeHistory::StreamKey).eq(any(stream_key_array(stream_keys))))
        .to_owned()
}

/// The distinct write identities of one temporal entity's Scope history.
fn scope_writes_query(schema: Option<&DynIden>, entity_key: TemporalEntityKey) -> SelectStatement {
    Query::select()
        .distinct()
        .column(ScopeHistory::WriteId)
        .from(qualified(schema, ScopeHistory::Table))
        .and_where(Expr::col(ScopeHistory::TemporalEntityKey).eq(entity_key.get()))
        .to_owned()
}

/// Remove one temporal entity's Scope history.
fn scope_history_delete(schema: Option<&DynIden>, entity_key: TemporalEntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, ScopeHistory::Table))
        .and_where(Expr::col(ScopeHistory::TemporalEntityKey).eq(entity_key.get()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::history_removal::{scope_history_delete, scope_writes_query, stream_history_delete, stream_writes_query};
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    fn key() -> TemporalEntityKey {
        TemporalEntityKey::try_from(7).expect("a positive key")
    }

    #[test]
    fn a_stream_removal_binds_the_streams_as_values_and_returns_nothing() {
        let sql = stream_history_delete(None, &[1, 2]).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "attribute_history" WHERE "stream_key" = ANY(ARRAY [1,2])"#);
    }

    #[test]
    fn a_stream_removal_reads_each_write_identity_once_over_the_same_streams() {
        let sql = stream_writes_query(None, &[1, 2]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT DISTINCT "write_id" FROM "attribute_history" WHERE "stream_key" = ANY(ARRAY [1,2])"#
        );
    }

    #[test]
    fn a_scope_removal_binds_the_entity_and_returns_nothing() {
        let delete = scope_history_delete(None, key()).to_string(PostgresQueryBuilder);
        let writes = scope_writes_query(None, key()).to_string(PostgresQueryBuilder);
        assert_eq!(delete, r#"DELETE FROM "scope_history" WHERE "temporal_entity_key" = 7"#);
        assert_eq!(writes, r#"SELECT DISTINCT "write_id" FROM "scope_history" WHERE "temporal_entity_key" = 7"#);
    }

    #[test]
    fn a_removal_addresses_the_rows_of_the_named_branch_schema() {
        let schema: DynIden = Alias::new("snap_7").into_iden();
        for sql in [
            stream_history_delete(Some(&schema), &[1]).to_string(PostgresQueryBuilder),
            stream_writes_query(Some(&schema), &[1]).to_string(PostgresQueryBuilder),
            scope_history_delete(Some(&schema), key()).to_string(PostgresQueryBuilder),
            scope_writes_query(Some(&schema), key()).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""snap_7"."#), "{sql}");
        }
    }
}
