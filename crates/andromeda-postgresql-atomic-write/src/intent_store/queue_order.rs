//! The statements that keep each Entity's queued temporal history in the order
//! its writes committed.
//!
//! Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in commit order, and
//! an intent's key is monotonic in commit order among the writes of one
//! Entity, which the Entity row lock serialises. An application refuses a set
//! holding an intent behind an earlier one of its Entity still queued outside
//! it, a claim takes only what such an application would accept, and a live
//! commit records inline only the Entities with no queued history. Every
//! statement naming one Entity's intents is answered by
//! `idx_temporal_append_intents_entity (entity_id, intent_key)`.

use crate::intent_store::{PostgresTemporalIntentStore, persistence};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::{
    literal::int_literal,
    pg_array::{all, any, is_any_of},
};
use andromeda_temporal::error::TemporalError;
use sea_query::{Cond, Expr, ExprTrait, Iden, JoinType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgConnection;
use std::collections::HashSet;

/// The alias of a claimed intent, `c`.
#[derive(Iden, Clone, Copy)]
#[iden = "c"]
pub(super) struct ClaimedIntent;

/// The alias of an earlier intent of the claimed intent's Entity, `p`.
#[derive(Iden, Clone, Copy)]
#[iden = "p"]
struct EarlierIntent;

/// What makes an earlier intent of an Entity keep a claim from taking the
/// Entity's later ones.
#[derive(Clone, Copy, Debug)]
pub(super) enum HeadBlock {
    /// A live lease, or a retry delay that has not passed: the background
    /// drain waits for both.
    LeaseOrBackoff,
    /// A live lease only: the mutation barrier ignores the retry delay.
    Lease,
}

/// Whether an earlier intent of the Entity of the intent under `c` is queued
/// and blocks it as `block` says.
///
/// Claims order by key under a limit, so with this filter a claim takes the
/// head of an Entity's queued intents and the run after it.
pub(super) fn earlier_intent_blocks(block: HeadBlock) -> SimpleExpr {
    let leased = Expr::col((EarlierIntent, TemporalAppendIntents::LeaseUntil)).gt(Expr::current_timestamp());
    let blocking = match block {
        HeadBlock::LeaseOrBackoff => leased.or(Expr::col((EarlierIntent, TemporalAppendIntents::NextAttemptAt)).gt(Expr::current_timestamp())),
        HeadBlock::Lease => leased,
    };
    let mut earlier = Query::select();
    earlier
        .expr(int_literal(1))
        .from_as(TemporalAppendIntents::Table, EarlierIntent)
        .and_where(Expr::col((EarlierIntent, TemporalAppendIntents::EntityId)).equals((ClaimedIntent, TemporalAppendIntents::EntityId)))
        .and_where(Expr::col((EarlierIntent, TemporalAppendIntents::IntentKey)).lt(Expr::col((ClaimedIntent, TemporalAppendIntents::IntentKey))))
        .and_where(blocking);
    Expr::exists(earlier)
}

/// Whether an intent named by `intent_keys` has an earlier intent of its
/// Entity queued outside them.
///
/// The queue removal runs inside the applying transaction, so an earlier
/// intent another transaction is applying but has not committed is still seen
/// queued: under READ COMMITTED the statement sees either that application
/// committed, with the row gone, or the row.
pub(super) fn behind_earlier_statement(intent_keys: &[i64]) -> SelectStatement {
    let mut behind = Query::select();
    behind
        .expr(int_literal(1))
        .from_as(TemporalAppendIntents::Table, ClaimedIntent)
        .join_as(
            JoinType::Join,
            TemporalAppendIntents::Table,
            EarlierIntent,
            Cond::all()
                .add(Expr::col((EarlierIntent, TemporalAppendIntents::EntityId)).equals((ClaimedIntent, TemporalAppendIntents::EntityId)))
                .add(Expr::col((EarlierIntent, TemporalAppendIntents::IntentKey)).lt(Expr::col((ClaimedIntent, TemporalAppendIntents::IntentKey)))),
        )
        .and_where(Expr::col((ClaimedIntent, TemporalAppendIntents::IntentKey)).eq(any(Expr::val(intent_keys.to_vec()))))
        .and_where(Expr::col((EarlierIntent, TemporalAppendIntents::IntentKey)).ne(all(Expr::val(intent_keys.to_vec()))));
    Query::select().expr(Expr::exists(behind)).to_owned()
}

/// The Entities among `entity_ids` with at least one intent queued.
pub(super) fn queued_entities_statement(entity_ids: &[&Identifier]) -> SelectStatement {
    Query::select()
        .distinct()
        .column(TemporalAppendIntents::EntityId)
        .from(TemporalAppendIntents::Table)
        .and_where(is_any_of(
            Expr::col(TemporalAppendIntents::EntityId),
            entity_ids.iter().map(|id| id.as_str().to_owned()),
        ))
        .to_owned()
}

impl PostgresTemporalIntentStore {
    /// The Entities among `entity_ids` whose history is queued, read in the
    /// caller's transaction.
    ///
    /// Read after the commit has written the Entities' rows, it is exact: the
    /// row lock is held, so every earlier writer of each Entity has committed
    /// its intents, and an intent a drain is applying but has not committed
    /// is still seen queued.
    pub(crate) async fn queued_entities_in_tx(connection: &mut PgConnection, entity_ids: &[&Identifier]) -> Result<HashSet<Identifier>, TemporalError> {
        if entity_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let (sql, values) = queued_entities_statement(entity_ids).build_sqlx(PostgresQueryBuilder);
        let queued: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(connection)
            .await
            .map_err(|error| persistence(&error))?;
        queued
            .iter()
            .map(|raw| {
                raw.parse::<Identifier>()
                    .map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::intent_store::queue_order::{HeadBlock, behind_earlier_statement, earlier_intent_blocks, queued_entities_statement};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use sea_query::{PostgresQueryBuilder, Query};

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    #[test]
    fn the_guard_looks_for_an_earlier_intent_of_the_same_entity_outside_the_set() {
        let sql = behind_earlier_statement(&[3, 5]).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("SELECT EXISTS(SELECT 1"), "{sql}");
        assert!(
            sql.contains(r#"FROM "temporal_append_intents" AS "c" JOIN "temporal_append_intents" AS "p""#),
            "{sql}"
        );
        assert!(sql.contains(r#""p"."entity_id" = "c"."entity_id""#), "{sql}");
        assert!(sql.contains(r#""p"."intent_key" < "c"."intent_key""#), "{sql}");
        assert!(sql.contains(r#""c"."intent_key" = ANY(ARRAY [3,5])"#), "{sql}");
        assert!(sql.contains(r#""p"."intent_key" <> ALL(ARRAY [3,5])"#), "{sql}");
    }

    #[test]
    fn the_queued_entities_probe_reads_each_entity_once() {
        let first = vehicle(1);
        let second = vehicle(2);
        let sql = queued_entities_statement(&[&first, &second]).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT DISTINCT "entity_id" FROM "temporal_append_intents""#), "{sql}");
        assert!(
            sql.contains(r#""entity_id" = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2'])"#),
            "{sql}"
        );
    }

    fn blocking(block: HeadBlock) -> String {
        Query::select().expr(earlier_intent_blocks(block)).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_background_drain_waits_behind_a_leased_or_backing_off_head() {
        let sql = blocking(HeadBlock::LeaseOrBackoff);
        assert!(
            sql.contains(r#"("p"."lease_until" > CURRENT_TIMESTAMP OR "p"."next_attempt_at" > CURRENT_TIMESTAMP)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""p"."entity_id" = "c"."entity_id""#), "{sql}");
        assert!(sql.contains(r#""p"."intent_key" < "c"."intent_key""#), "{sql}");
    }

    #[test]
    fn the_barrier_waits_behind_a_leased_head_only() {
        let sql = blocking(HeadBlock::Lease);
        assert!(sql.contains(r#""p"."lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!sql.contains("next_attempt_at"), "{sql}");
    }
}
