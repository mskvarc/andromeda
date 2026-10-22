//! Removing the families and value shapes a stream no longer holds.
//!
//! The write path adds a combination of family and shape to
//! `temporal_stream_shapes` when it records an instance of one its stream
//! does not hold yet, and never removes one. A path that removes or modifies
//! recorded instances (a clause 5.6.14 modification, a clause 5.6.15
//! deletion, a client `instanceId` a later write replaces under clause 4.5.7,
//! a windowed deletion, operator retention) prunes the streams it touched: a
//! combination stays while one instance of the stream still carries it and
//! goes with the last, so the catalogue answers clause 5.7.4.4 as the
//! instances that exist do.
//!
//! A prune reads what its streams hold only once it holds the row lock of each
//! of their Temporal Evolutions, taken in a statement of its own so that,
//! under read committed, the read starts after the lock is granted. Every
//! write recording an instance takes that lock first, in its evolution upsert,
//! and keeps it to its commit. A write recording a combination the prune is
//! about to remove has therefore either committed, and the prune reads its
//! instance, or waits until the prune has committed, and then adds the
//! combination again.

use crate::{error::TemporalStoreError, history_removal::stream_key_array, repo::TemporalRepository, statement_timeout_allowance::allow_unbounded_statements};
use andromeda_postgresql_query::tombstone::carries_a_value;
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities, TemporalStreamShapes};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::any, statement::Statement};
use andromeda_postgresql_temporal_query::{alias::Ah, recorded_shape::recorded_shape};
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, LockBehavior, LockType, Order, Query, SelectStatement};
use sqlx::PgConnection;

/// How many Temporal Evolutions one retention prune transaction locks: enough
/// to amortise the round trips, few enough that a write waiting on one of them
/// waits for a short transaction.
const RETENTION_PRUNE_BATCH: u64 = 500;

/// Lock the Temporal Evolution of every stream `stream_keys` names, in key
/// order, until the transaction ends.
///
/// A path that also releases an instance claim takes it before anything else:
/// a write replacing that instance holds its evolution lock while it waits on
/// the claim, so taking the lock after the claim would deadlock against it.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn lock_stream_evolutions(connection: &mut PgConnection, schema: Option<&DynIden>, stream_keys: &[i64]) -> Result<(), TemporalStoreError> {
    if stream_keys.is_empty() {
        return Ok(());
    }
    evolution_lock_statement(schema, stream_keys).to_query().execute(connection).await?;
    Ok(())
}

/// Lock the Temporal Evolutions of `stream_keys`, then remove every
/// combination of family and shape no instance of its stream carries any
/// longer, returning how many were removed.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a statement fails.
pub(crate) async fn prune_stream_shapes(connection: &mut PgConnection, schema: Option<&DynIden>, stream_keys: &[i64]) -> Result<u64, TemporalStoreError> {
    if stream_keys.is_empty() {
        return Ok(0);
    }
    lock_stream_evolutions(&mut *connection, schema, stream_keys).await?;
    Ok(prune_statement(schema, stream_keys).to_query().execute(connection).await?.rows_affected())
}

impl TemporalRepository {
    /// Prune every stream of the live branch after retention dropped history,
    /// returning how many combinations were removed.
    ///
    /// Retention drops whole chunks across every stream, so every stream is
    /// pruned, a batch of Temporal Evolutions per transaction. An Evolution a
    /// write holds at that moment is skipped rather than waited for: waiting
    /// on locks taken in another order could deadlock against the write, and
    /// a skipped stream keeps a combination too many until the next sweep,
    /// which only ever refuses an aggregation, never admits one.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails.
    pub(crate) async fn prune_retained_stream_shapes(&self) -> Result<u64, TemporalStoreError> {
        let mut pruned = 0;
        let mut after = 0_i64;
        loop {
            let mut transaction = self.pool.begin().await?;
            allow_unbounded_statements(&mut transaction).await?;
            let evolutions: Vec<i64> = retention_batch_lock_statement(after).to_query_scalar().fetch_all(&mut *transaction).await?;
            let Some(last) = evolutions.last().copied() else {
                transaction.commit().await?;
                return Ok(pruned);
            };
            let streams: Vec<i64> = evolution_streams_statement(&evolutions).to_query_scalar().fetch_all(&mut *transaction).await?;
            if !streams.is_empty() {
                pruned += prune_statement(None, &streams).to_query().execute(&mut *transaction).await?.rows_affected();
            }
            transaction.commit().await?;
            after = last;
        }
    }
}

/// The Temporal Evolutions of `stream_keys`, locked for update in key order.
fn evolution_lock_statement(schema: Option<&DynIden>, stream_keys: &[i64]) -> SelectStatement {
    let owners = Query::select()
        .column(TemporalAttributeStreams::TemporalEntityKey)
        .from(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(any(stream_key_array(stream_keys))))
        .to_owned();
    Query::select()
        .column(TemporalEntities::TemporalEntityKey)
        .from(qualified(schema, TemporalEntities::Table))
        .and_where(Expr::col(TemporalEntities::TemporalEntityKey).in_subquery(owners))
        .order_by(TemporalEntities::TemporalEntityKey, Order::Asc)
        .lock(LockType::Update)
        .to_owned()
}

/// The next batch of live Temporal Evolutions after the key `after` that no
/// write holds, locked for update.
fn retention_batch_lock_statement(after: i64) -> SelectStatement {
    Query::select()
        .column(TemporalEntities::TemporalEntityKey)
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::TemporalEntityKey).gt(after))
        .order_by(TemporalEntities::TemporalEntityKey, Order::Asc)
        .limit(RETENTION_PRUNE_BATCH)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned()
}

/// The live streams of the Temporal Evolutions `evolutions` names.
fn evolution_streams_statement(evolutions: &[i64]) -> SelectStatement {
    Query::select()
        .column(TemporalAttributeStreams::StreamKey)
        .from(TemporalAttributeStreams::Table)
        .and_where(Expr::col(TemporalAttributeStreams::TemporalEntityKey).eq(any(Expr::val(evolutions.to_vec()))))
        .to_owned()
}

/// Remove the combinations of `stream_keys` no instance of their stream
/// carries.
///
/// The probe stops at the first instance carrying the combination, so a
/// combination still held costs one row read. It binds the streams inside as
/// well: correlated alone, the planner reads every history row of the relation
/// to hash it.
fn prune_statement(schema: Option<&DynIden>, stream_keys: &[i64]) -> DeleteStatement {
    let shapes = |column: TemporalStreamShapes| Expr::col((TemporalStreamShapes::Table, column));
    let held = Query::select()
        .expr(Expr::val(1))
        .from(qualified(schema, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).eq(any(stream_key_array(stream_keys))))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).eq(shapes(TemporalStreamShapes::StreamKey)))
        .and_where(Expr::col((Ah, AttributeHistory::AttributeKind)).eq(shapes(TemporalStreamShapes::AttributeKind)))
        .and_where(carries_a_value(Ah))
        .and_where(recorded_shape().eq(shapes(TemporalStreamShapes::RecordedShape)))
        .to_owned();
    Query::delete()
        .from_table(qualified(schema, TemporalStreamShapes::Table))
        .and_where(Expr::col(TemporalStreamShapes::StreamKey).eq(any(stream_key_array(stream_keys))))
        .and_where(Expr::exists(held).not())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::stream_shape_pruning::{evolution_lock_statement, evolution_streams_statement, prune_statement, retention_batch_lock_statement};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    /// A combination goes only when no instance of its stream carries it: the
    /// probe compares the family and the refined shape of every row of the
    /// pruned streams that is not a tombstone.
    #[test]
    fn a_combination_goes_only_when_no_instance_of_its_stream_carries_it() {
        let sql = prune_statement(None, &[4, 5]).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"DELETE FROM "temporal_stream_shapes" WHERE "stream_key" = ANY(ARRAY [4,5]) AND (NOT EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."stream_key" = ANY(ARRAY [4,5]) AND "ah"."stream_key" = "temporal_stream_shapes"."stream_key" AND "ah"."attribute_kind" = "temporal_stream_shapes"."attribute_kind" AND "ah"."value_kind" <> (0) AND (CASE"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"END) = "temporal_stream_shapes"."recorded_shape"))"#), "{sql}");
    }

    #[test]
    fn the_evolutions_of_the_pruned_streams_are_locked_in_key_order() {
        let sql = evolution_lock_statement(None, &[4]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "temporal_entity_key" FROM "temporal_entities" WHERE "temporal_entity_key" IN (SELECT "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "stream_key" = ANY(ARRAY [4])) ORDER BY "temporal_entity_key" ASC FOR UPDATE"#
        );
    }

    #[test]
    fn a_snapshot_branch_prunes_and_locks_its_own_relations() {
        let schema: DynIden = Alias::new("snap_7").into_iden();
        for sql in [
            prune_statement(Some(&schema), &[1]).to_string(PostgresQueryBuilder),
            evolution_lock_statement(Some(&schema), &[1]).to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains(r#"FROM "temporal_"#) && !sql.contains(r#"FROM "attribute_history""#), "{sql}");
            assert!(sql.contains(r#"FROM "snap_7"."#), "{sql}");
        }
    }

    /// Retention walks every Evolution once, in key order, skipping the ones a
    /// write holds rather than waiting for them.
    #[test]
    fn retention_prunes_in_batches_of_evolutions_no_write_holds() {
        let lock = retention_batch_lock_statement(41).to_string(PostgresQueryBuilder);
        assert_eq!(
            lock,
            r#"SELECT "temporal_entity_key" FROM "temporal_entities" WHERE "temporal_entity_key" > 41 ORDER BY "temporal_entity_key" ASC LIMIT 500 FOR UPDATE SKIP LOCKED"#
        );
        let streams = evolution_streams_statement(&[42, 43]).to_string(PostgresQueryBuilder);
        assert_eq!(
            streams,
            r#"SELECT "stream_key" FROM "temporal_attribute_streams" WHERE "temporal_entity_key" = ANY(ARRAY [42,43])"#
        );
    }
}
