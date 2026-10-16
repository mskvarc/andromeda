//! Removing the history rows an application's entries replace.
//!
//! An entry naming an instance that history already holds (a client-supplied
//! `instanceId`, clause 4.5.7, or a replayed Scope instance) replaces that row,
//! which may sit in a converted chunk. TimescaleDB narrows the compressed
//! batches an `UPDATE` or `DELETE` decompresses only by bound values on the
//! segmenting column and on the order column's min/max summary; a delete
//! correlated to the entries would have it decompress every batch of every
//! converted chunk. So the rows are found by a read, which the join narrows
//! through the compressed segment index, and removed by a delete that binds
//! their segments, the span of their instants and their exact identities.

use crate::{decompression_allowance::allow_confined_decompression, error::TemporalStoreError};
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    pg_array::any,
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{DynIden, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::{SqlxBinder, SqlxValues};
use sqlx::{AssertSqlSafe, PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

/// The `UNNEST` source of the replaced identities.
#[derive(Iden, Clone, Copy)]
#[iden = "replaced"]
struct Replaced;

/// The replaced row's `recorded_at` in the identity source.
#[derive(Iden, Clone, Copy)]
#[iden = "recorded_at"]
struct ReplacedRecordedAt;

/// The replaced row's `write_id` in the identity source.
#[derive(Iden, Clone, Copy)]
#[iden = "write_id"]
struct ReplacedWriteId;

/// The replaced row's `write_ordinal` in the identity source.
#[derive(Iden, Clone, Copy)]
#[iden = "write_ordinal"]
struct ReplacedWriteOrdinal;

/// The history relation a replacement removes rows from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplacedHistory {
    /// `attribute_history`, segmented by stream.
    AttributeInstances,
    /// `scope_history`, segmented by temporal entity.
    ScopeInstances,
}

/// One history row a replacement removes: its segment and its write identity.
///
/// The identity read projects these four columns in this order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReplacedRow {
    /// The stream or temporal entity key the row is segmented by.
    segment: i64,
    /// The row's write clock.
    recorded_at: DateTime<Utc>,
    /// The write the row was recorded by.
    write_id: Uuid,
    /// The row's position within its write.
    write_ordinal: i32,
}

impl ReplacedRow {
    /// Decode one row of an identity read.
    fn of(row: &PgRow) -> Result<ReplacedRow, sqlx::Error> {
        Ok(ReplacedRow {
            segment: row.try_get(0)?,
            recorded_at: row.try_get(1)?,
            write_id: row.try_get(2)?,
            write_ordinal: row.try_get(3)?,
        })
    }
}

impl ReplacedHistory {
    /// The segmenting column of the relation.
    fn segment(self) -> SimpleExpr {
        match self {
            ReplacedHistory::AttributeInstances => Expr::col(AttributeHistory::StreamKey),
            ReplacedHistory::ScopeInstances => Expr::col(ScopeHistory::TemporalEntityKey),
        }
    }

    /// The delete of exactly `rows`, which must not be empty.
    ///
    /// Both relations name their identity columns alike, so the
    /// `attribute_history` names stand for either.
    fn delete_statement(self, schema: Option<&DynIden>, rows: &[ReplacedRow]) -> (String, SqlxValues) {
        let mut statement = match self {
            ReplacedHistory::AttributeInstances => Query::delete().from_table(qualified(schema, AttributeHistory::Table)).to_owned(),
            ReplacedHistory::ScopeInstances => Query::delete().from_table(qualified(schema, ScopeHistory::Table)).to_owned(),
        };
        let segments = rows.iter().map(|row| row.segment).collect::<Vec<i64>>();
        let instants = rows.iter().map(|row| row.recorded_at).collect::<Vec<DateTime<Utc>>>();
        let earliest = instants.iter().min().copied();
        let latest = instants.iter().max().copied();
        let columns = identity_columns();
        let identities = Query::select()
            .column((Replaced, ReplacedRecordedAt))
            .column((Replaced, ReplacedWriteId))
            .column((Replaced, ReplacedWriteOrdinal))
            .from_function(
                unnest_call([
                    (&columns[0], Expr::val(instants)),
                    (&columns[1], Expr::val(rows.iter().map(|row| row.write_id).collect::<Vec<Uuid>>())),
                    (&columns[2], Expr::val(rows.iter().map(|row| row.write_ordinal).collect::<Vec<i32>>())),
                ]),
                Replaced,
            )
            .to_owned();
        statement.and_where(self.segment().eq(any(Expr::val(segments))));
        if let (Some(earliest), Some(latest)) = (earliest, latest) {
            statement
                .and_where(Expr::col(AttributeHistory::RecordedAt).gte(earliest))
                .and_where(Expr::col(AttributeHistory::RecordedAt).lte(latest));
        }
        statement.and_where(
            Expr::tuple([
                Expr::col(AttributeHistory::RecordedAt),
                Expr::col(AttributeHistory::WriteId),
                Expr::col(AttributeHistory::WriteOrdinal),
            ])
            .in_subquery(identities),
        );
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        (apply_column_alias(sql, &Replaced, &columns), values)
    }
}

/// The columns of the identity source, in projection order.
fn identity_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(ReplacedRecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(ReplacedWriteId, PostgresType::UuidArray),
        UnnestColumn::new(ReplacedWriteOrdinal, PostgresType::IntegerArray),
    ]
}

/// Remove the rows the identity read `rows` returned from `history`.
///
/// A replaced row's batch is decompressed whole, so the transaction lifts the
/// decompression limit before the delete: the bound values already confine it
/// to the batches holding the replaced rows.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a row does not decode or a
/// statement fails.
pub(crate) async fn remove_replaced(
    connection: &mut PgConnection,
    schema: Option<&DynIden>,
    history: ReplacedHistory,
    rows: &[PgRow],
) -> Result<(), TemporalStoreError> {
    let replaced = rows.iter().map(ReplacedRow::of).collect::<Result<Vec<ReplacedRow>, sqlx::Error>>()?;
    if replaced.is_empty() {
        return Ok(());
    }
    allow_confined_decompression(connection).await?;
    let (sql, values) = history.delete_statement(schema, &replaced);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::intent_application::replaced_rows::{ReplacedHistory, ReplacedRow};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden};
    use uuid::Uuid;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn rows() -> Vec<ReplacedRow> {
        vec![
            ReplacedRow {
                segment: 11,
                recorded_at: instant("2026-05-02T00:00:00Z"),
                write_id: Uuid::nil(),
                write_ordinal: 3,
            },
            ReplacedRow {
                segment: 12,
                recorded_at: instant("2026-04-01T00:00:00Z"),
                write_id: Uuid::max(),
                write_ordinal: 0,
            },
        ]
    }

    #[test]
    fn a_replaced_instance_is_removed_by_its_stream_its_instant_span_and_its_exact_identity() {
        let (sql, values) = ReplacedHistory::AttributeInstances.delete_statement(None, &rows());
        assert_eq!(
            sql,
            r#"DELETE FROM "attribute_history" WHERE "stream_key" = ANY($1) AND "recorded_at" >= $2 AND "recorded_at" <= $3 AND ("recorded_at", "write_id", "write_ordinal") IN (SELECT "replaced"."recorded_at", "replaced"."write_id", "replaced"."write_ordinal" FROM UNNEST($4::timestamptz[], $5::uuid[], $6::integer[]) AS replaced(recorded_at, write_id, write_ordinal))"#
        );
        let bound = format!("{values:?}");
        assert!(bound.contains("2026-04-01") && bound.contains("2026-05-02"), "{bound}");
        assert_eq!(values.0.0.len(), 6, "{bound}");
    }

    #[test]
    fn a_replaced_scope_instance_is_removed_by_its_temporal_entity() {
        let (sql, _) = ReplacedHistory::ScopeInstances.delete_statement(None, &rows());
        assert!(sql.starts_with(r#"DELETE FROM "scope_history" WHERE "temporal_entity_key" = ANY($1)"#), "{sql}");
    }

    #[test]
    fn a_replacement_removes_the_rows_of_the_addressed_branch_schema() {
        let schema: DynIden = Alias::new("snap_3").into_iden();
        let (sql, _) = ReplacedHistory::AttributeInstances.delete_statement(Some(&schema), &rows());
        assert!(sql.starts_with(r#"DELETE FROM "snap_3"."attribute_history""#), "{sql}");
    }
}
