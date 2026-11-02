//! Removing every history row of whole streams, or of one temporal entity's
//! Scope.
//!
//! On a converted chunk TimescaleDB removes a compressed batch without
//! decompressing it only when every predicate binds the segmenting column to a
//! value and the statement returns nothing; anything else decompresses each
//! batch it reaches, row by row, under a per-transaction limit that a long
//! stream exceeds. So the segment is bound as values here, never as a
//! subquery.

use crate::error::TemporalStoreError;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::is_any_of, statement::Statement};
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, Query};
use sqlx::{Postgres, Transaction};

/// Remove every history row of `stream_keys`, returning how many it removed.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn remove_stream_history(
    tx: &mut Transaction<'_, Postgres>,
    schema: Option<&DynIden>,
    stream_keys: &[i64],
) -> Result<u64, TemporalStoreError> {
    Ok(stream_history_delete(schema, stream_keys).to_query().execute(&mut **tx).await?.rows_affected())
}

/// Remove every Scope history row of the temporal entity `entity_key`,
/// returning how many it removed.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn remove_scope_history(
    tx: &mut Transaction<'_, Postgres>,
    schema: Option<&DynIden>,
    entity_key: EntityKey,
) -> Result<u64, TemporalStoreError> {
    Ok(scope_history_delete(schema, entity_key).to_query().execute(&mut **tx).await?.rows_affected())
}

/// Remove the history rows of `stream_keys`.
fn stream_history_delete(schema: Option<&DynIden>, stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, AttributeHistory::Table))
        .and_where(is_any_of(Expr::col(AttributeHistory::StreamKey), stream_keys.iter().copied()))
        .to_owned()
}

/// Remove one temporal entity's Scope history.
fn scope_history_delete(schema: Option<&DynIden>, entity_key: EntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, ScopeHistory::Table))
        .and_where(Expr::col(ScopeHistory::EntityKey).eq(entity_key.get()))
        .to_owned()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_removal::{scope_history_delete, stream_history_delete};
    use andromeda_postgresql_codes::entity_key::EntityKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    fn key() -> EntityKey {
        EntityKey::try_from(7).expect("a positive key")
    }

    #[test]
    fn a_stream_removal_binds_the_streams_as_values_and_returns_nothing() {
        let sql = stream_history_delete(None, &[1, 2]).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "attribute_history" WHERE "stream_key" = ANY(ARRAY [1,2])"#);
    }

    #[test]
    fn a_scope_removal_binds_the_entity_and_returns_nothing() {
        let delete = scope_history_delete(None, key()).to_string(PostgresQueryBuilder);
        assert_eq!(delete, r#"DELETE FROM "scope_history" WHERE "entity_key" = 7"#);
    }

    #[test]
    fn a_removal_addresses_the_rows_of_the_named_branch_schema() {
        let schema: DynIden = Alias::new("snap_7").into_iden();
        for sql in [
            stream_history_delete(Some(&schema), &[1]).to_string(PostgresQueryBuilder),
            scope_history_delete(Some(&schema), key()).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""snap_7"."#), "{sql}");
        }
    }
}
