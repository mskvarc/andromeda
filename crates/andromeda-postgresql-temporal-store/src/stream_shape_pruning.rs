//! Clearing the families and value shapes a stream no longer holds from its
//! shape mask.
//!
//! The write path sets the bit of a combination of family and shape in the
//! stream's mask when it records an instance of one the mask lacks, and never
//! clears one. A path that removes or modifies
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

use crate::{error::TemporalStoreError, repo::TemporalRepository, statement_timeout_allowance::allow_unbounded_statements};
use andromeda_postgresql_query::{entity_sides::holds_evolution, tombstone::carries_a_value};
use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    bit_string::bit_union_of_rows,
    branch::qualified,
    literal::int_literal,
    pg_array::is_any_of,
    scalar_subquery::scalar_subquery,
    statement::Statement,
};
use andromeda_postgresql_temporal_query::{
    alias::Ah,
    shape_mask::{held_positions, no_shapes, position_mask, recorded_combination},
};
use sea_query::{DynIden, Expr, ExprTrait, Func, Iden, IntoIden, LockBehavior, LockType, Order, Query, SelectStatement, TableRef, UpdateStatement};
use sqlx::PgConnection;

/// Alias `ps`: a stream whose mask is pruned.
#[derive(Iden, Clone, Copy)]
#[iden = "ps"]
struct Pruned;

/// Alias `kept`: the mask each pruned stream keeps.
#[derive(Iden, Clone, Copy)]
#[iden = "kept"]
struct Kept;

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

/// Lock the Temporal Evolutions of `stream_keys`, then clear from their masks
/// every combination of family and shape no instance of the stream carries
/// any longer, returning how many masks lost one.
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
    /// returning how many masks lost a combination.
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
        .column(TemporalAttributeStreams::EntityKey)
        .from(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(is_any_of(Expr::col(TemporalAttributeStreams::StreamKey), stream_keys.iter().copied()))
        .to_owned();
    Query::select()
        .column(Entities::EntityKey)
        .from(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(owners))
        .order_by(Entities::EntityKey, Order::Asc)
        .lock(LockType::Update)
        .to_owned()
}

/// The next batch of live Temporal Evolutions after the key `after` that no
/// write holds, locked for update.
fn retention_batch_lock_statement(after: i64) -> SelectStatement {
    Query::select()
        .column(Entities::EntityKey)
        .from(Entities::Table)
        .and_where(Expr::col(Entities::EntityKey).gt(after))
        .and_where(holds_evolution(Entities::Table))
        .order_by(Entities::EntityKey, Order::Asc)
        .limit(RETENTION_PRUNE_BATCH)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned()
}

/// The live streams of the Temporal Evolutions `evolutions` names.
fn evolution_streams_statement(evolutions: &[i64]) -> SelectStatement {
    Query::select()
        .column(TemporalAttributeStreams::StreamKey)
        .from(TemporalAttributeStreams::Table)
        .and_where(is_any_of(Expr::col(TemporalAttributeStreams::EntityKey), evolutions.iter().copied()))
        .to_owned()
}

/// Clear from the masks of `stream_keys` the combinations no instance of
/// their stream carries.
///
/// Each combination a mask holds is probed on its own, and the probe stops at
/// the first instance carrying it, so a combination still held costs one row
/// read. The probe binds the streams inside as well: correlated alone, the
/// planner reads every history row of the relation to hash it. A mask that
/// keeps every combination is left alone.
fn prune_statement(schema: Option<&DynIden>, stream_keys: &[i64]) -> UpdateStatement {
    let carried = Query::select()
        .expr(int_literal(1))
        .from(qualified(schema, AttributeHistory::Table).alias(Ah))
        .and_where(is_any_of(Expr::col((Ah, AttributeHistory::StreamKey)), stream_keys.iter().copied()))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).equals((Pruned, TemporalAttributeStreams::StreamKey)))
        .and_where(carries_a_value(Ah))
        .and_where(recorded_combination().eq(position_mask()))
        .take();
    let kept = held_positions(Expr::col((Pruned, TemporalAttributeStreams::ShapeMask)))
        .expr(bit_union_of_rows(position_mask()))
        .and_where(Expr::exists(carried))
        .take();
    let masks = Query::select()
        .column((Pruned, TemporalAttributeStreams::StreamKey))
        .expr_as(Func::coalesce([scalar_subquery(kept), no_shapes()]), TemporalAttributeStreams::ShapeMask)
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(Pruned))
        .and_where(is_any_of(Expr::col((Pruned, TemporalAttributeStreams::StreamKey)), stream_keys.iter().copied()))
        .take();
    let mask = || Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::ShapeMask));
    Query::update()
        .table(qualified(schema, TemporalAttributeStreams::Table))
        .value(TemporalAttributeStreams::ShapeMask, Expr::col((Kept, TemporalAttributeStreams::ShapeMask)))
        .from(TableRef::SubQuery(Box::new(masks), Kept.into_iden()))
        .and_where(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((Kept, TemporalAttributeStreams::StreamKey)))
        .and_where(mask().ne(Expr::col((Kept, TemporalAttributeStreams::ShapeMask))))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::stream_shape_pruning::{evolution_lock_statement, evolution_streams_statement, prune_statement, retention_batch_lock_statement};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    /// A combination goes only when no instance of its stream carries it:
    /// each held bit is probed against the family and the refined shape of
    /// the rows of the pruned streams that are not tombstones, and a mask is
    /// rewritten only when it loses one.
    #[test]
    fn a_combination_goes_only_when_no_instance_of_its_stream_carries_it() {
        let sql = prune_statement(None, &[4, 5]).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(concat!(
                r#"UPDATE "temporal_attribute_streams" SET "shape_mask" = "kept"."shape_mask""#,
                r#" FROM (SELECT "ps"."stream_key", COALESCE((SELECT bit_or((B'1'::bit(80) >> ("position")))"#,
                r#" FROM generate_series(0, 79) AS "position" WHERE get_bit("ps"."shape_mask", "position") = 1"#,
                r#" AND EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."stream_key" = ANY(ARRAY [4,5]) AND "ah"."stream_key" = "ps"."stream_key" AND "ah"."value_kind" <> (0)"#
            )),
            "{sql}"
        );
        assert!(
            sql.ends_with(concat!(
                r#"B'0'::bit(80)) AS "shape_mask" FROM "temporal_attribute_streams" AS "ps" WHERE "ps"."stream_key" = ANY(ARRAY [4,5])) AS "kept""#,
                r#" WHERE "temporal_attribute_streams"."stream_key" = "kept"."stream_key" AND "temporal_attribute_streams"."shape_mask" <> "kept"."shape_mask""#
            )),
            "{sql}"
        );
    }

    #[test]
    fn the_evolutions_of_the_pruned_streams_are_locked_in_key_order() {
        let sql = evolution_lock_statement(None, &[4]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "entity_key" FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "temporal_attribute_streams" WHERE "stream_key" = 4) ORDER BY "entity_key" ASC FOR UPDATE"#
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
            r#"SELECT "entity_key" FROM "entities" WHERE "entity_key" > 41 AND "entities"."evolution_created_at" IS NOT NULL ORDER BY "entity_key" ASC LIMIT 500 FOR UPDATE SKIP LOCKED"#
        );
        let streams = evolution_streams_statement(&[42, 43]).to_string(PostgresQueryBuilder);
        assert_eq!(
            streams,
            r#"SELECT "stream_key" FROM "temporal_attribute_streams" WHERE "entity_key" = ANY(ARRAY [42,43])"#
        );
    }
}
