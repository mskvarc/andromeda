//! PostgreSQL anchor persistence and delivery state for temporal append intents.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::{
    pg_array::text_array,
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_temporal::{
    append_intent::{SequencedTemporalAppendIntent, TemporalAppendIntent, TemporalIntentSequence},
    error::TemporalError,
    port::{
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{
    Alias,
    DeleteStatement,
    Expr,
    ExprTrait,
    InsertStatement,
    Keyword,
    LockBehavior,
    LockType,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// The `AS input(write_id, entity_id, base, intent)` column-list alias the
/// enqueue's `UNNEST` set-write projects, in the order the parallel arrays are
/// bound and the insert's columns are named.
fn intent_unnest_columns() -> [UnnestColumn; 4] {
    [
        UnnestColumn::new(Alias::new("write_id"), PostgresType::UuidArray),
        UnnestColumn::new(Alias::new("entity_id"), PostgresType::TextArray),
        UnnestColumn::new(Alias::new("base"), PostgresType::IntegerArray),
        UnnestColumn::new(Alias::new("intent"), PostgresType::JsonbArray),
    ]
}

/// Insert a batch of temporal append intents from four parallel arrays.
///
/// One commit may land more than one entity, and every intent it produces
/// shares the commit's single write identity; that alone does not identify
/// one durable intent, because a batch commit in
/// `BatchSideEffectMode::Enqueue` mode writes an inline intent for an
/// entity's tombstones and later replays that same entity's deferred
/// side-effect job under the same write identity, producing a second intent
/// for the same entity whose entries occupy a different ordinal range. The
/// deduplicating key is therefore `(write_id, entity_id, base)`, matching
/// `uq_temporal_append_intents_write`, rather than `(write_id, entity_id)`.
/// `ON CONFLICT ... DO NOTHING` is what makes a commit replayed under its
/// original identity enqueue no second copy of any entity's intent at a given
/// base.
fn enqueue_statement(ids: Vec<Uuid>, entity_ids: &[&str], bases: Vec<i32>, payloads: Vec<Value>) -> Result<InsertStatement, sea_query::error::Error> {
    let columns = intent_unnest_columns();
    let call = unnest_call([
        (&columns[0], Expr::val(ids)),
        (&columns[1], text_array(entity_ids)),
        (&columns[2], Expr::val(bases)),
        (&columns[3], Expr::val(payloads)),
    ]);
    let input = Alias::new("input");
    let source = Query::select()
        .expr(Expr::col((input.clone(), Alias::new("write_id"))))
        .expr(Expr::col((input.clone(), Alias::new("entity_id"))))
        .expr(Expr::col((input.clone(), Alias::new("base"))))
        .expr(Expr::col((input.clone(), Alias::new("intent"))))
        .from_function(call, input)
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(TemporalAppendIntents::Table)
        .columns([
            TemporalAppendIntents::WriteId,
            TemporalAppendIntents::EntityId,
            TemporalAppendIntents::Base,
            TemporalAppendIntents::Intent,
        ])
        .select_from(source)?
        .on_conflict(
            sea_query::OnConflict::columns([TemporalAppendIntents::WriteId, TemporalAppendIntents::EntityId, TemporalAppendIntents::Base])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement.to_owned())
}

/// PostgreSQL-backed pending temporal intent queue for one tenant.
#[derive(Clone, Debug)]
pub struct PostgresTemporalIntentStore {
    /// Tenant anchor pool.
    pool: PgPool,
}

impl PostgresTemporalIntentStore {
    /// Construct a pending-intent store over the tenant anchor pool.
    pub fn new(pool: &PgPool) -> PostgresTemporalIntentStore {
        PostgresTemporalIntentStore {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }

    /// Persist complete intents inside the entity anchor transaction.
    pub(crate) async fn enqueue_in_tx(tx: &mut Transaction<'_, Postgres>, intents: &[TemporalAppendIntent]) -> Result<(), TemporalError> {
        if intents.is_empty() {
            return Ok(());
        }
        let mut ids = Vec::with_capacity(intents.len());
        let mut entity_ids = Vec::with_capacity(intents.len());
        let mut bases = Vec::with_capacity(intents.len());
        let mut payloads = Vec::with_capacity(intents.len());
        for intent in intents {
            ids.push(intent.record.write_id().as_uuid());
            entity_ids.push(intent.entity_id.as_str());
            bases.push(
                intent
                    .base
                    .as_i32()
                    .map_err(|error| TemporalError::internal(format!("temporal append intent base is out of range: {error}")))?,
            );
            payloads.push(serde_json::to_value(intent).map_err(|error| TemporalError::internal(format!("failed to encode temporal append intent: {error}")))?);
        }
        let statement = enqueue_statement(ids, &entity_ids, bases, payloads)
            .map_err(|error| TemporalError::internal(format!("failed to build intent enqueue: {error}")))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let sql = apply_column_alias(sql, &Alias::new("input"), &intent_unnest_columns());
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&mut **tx)
            .await
            .map_err(persistence)?;
        Ok(())
    }

    /// Decode one durable queue row.
    fn decode(row: &sqlx::postgres::PgRow) -> Result<SequencedTemporalAppendIntent, TemporalError> {
        let raw_sequence: i64 = row.try_get("sequence").map_err(persistence)?;
        let sequence = TemporalIntentSequence::new(raw_sequence).map_err(|error| TemporalError::internal(error.to_string()))?;
        let payload: Value = row.try_get("intent").map_err(persistence)?;
        let intent =
            serde_json::from_value(payload).map_err(|error| TemporalError::persistence(format!("failed to decode temporal append intent: {error}")))?;
        Ok(SequencedTemporalAppendIntent { sequence, intent })
    }

    /// Claim an ordered bounded batch, optionally restricted to one entity.
    async fn claim_matching(
        &self,
        owner: TemporalIntentLeaseOwner,
        entity_id: Option<&Uri>,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = claim_statement(owner.as_uuid(), entity_id.map(Uri::as_str), i64::from(limit.get()), lease_until).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        let mut claimed = rows
            .iter()
            .map(|row| {
                let attempts: i32 = row.try_get("attempts").map_err(persistence)?;
                let attempts = u32::try_from(attempts).map_err(|error| TemporalError::internal(format!("negative temporal intent attempt count: {error}")))?;
                Ok(ClaimedTemporalAppendIntent {
                    pending: PostgresTemporalIntentStore::decode(row)?,
                    attempts: attempts.saturating_sub(1),
                })
            })
            .collect::<Result<Vec<_>, TemporalError>>()?;
        claimed.sort_by_key(|claim| claim.pending.sequence);
        Ok(claimed)
    }
}

#[async_trait]
impl PendingTemporalIntentReader for PostgresTemporalIntentStore {
    async fn pending_for_entity(&self, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(Some(entity_id.as_str())).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }

    async fn pending_intents(&self) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        let (sql, values) = pending_statement(None).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence)?;
        rows.iter().map(PostgresTemporalIntentStore::decode).collect()
    }
}

#[async_trait]
impl TemporalIntentQueue for PostgresTemporalIntentStore {
    async fn claim(
        &self,
        owner: TemporalIntentLeaseOwner,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, None, limit, lease_until).await
    }

    async fn claim_for_entity(
        &self,
        owner: TemporalIntentLeaseOwner,
        entity_id: &Uri,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        self.claim_matching(owner, Some(entity_id), limit, lease_until).await
    }

    async fn acknowledge(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent) -> Result<(), TemporalError> {
        let (sql, values) =
            acknowledge_statement(intent.sequence.get(), intent.intent.record.write_id().as_uuid(), owner.as_uuid()).build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.pool)
            .await
            .map_err(persistence)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(TemporalError::internal("temporal intent acknowledgement did not own exactly one leased row"))
        }
    }

    async fn retry(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<(), TemporalError> {
        let (sql, values) = retry_statement(
            retry.eligible_at,
            &retry.summary,
            intent.sequence.get(),
            intent.intent.record.write_id().as_uuid(),
            owner.as_uuid(),
        )
        .build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.pool)
            .await
            .map_err(persistence)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(TemporalError::internal("temporal intent retry did not own exactly one leased row"))
        }
    }
}

/// Claim an ordered bounded batch of eligible intents for `owner`.
///
/// The original data-modifying CTE (`WITH eligible AS (SELECT … FOR UPDATE SKIP
/// LOCKED) UPDATE … RETURNING`) is rebuilt as one `UPDATE` whose `WHERE sequence
/// IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects and claims the batch
/// atomically. When `entity_id` is given, only that entity's intents are
/// eligible; the original expressed this with a `$4 IS NULL OR entity_id = $4`
/// guard, which is here simply an absent predicate.
fn claim_statement(owner: Uuid, entity_id: Option<&str>, limit: i64, lease_until: DateTime<Utc>) -> UpdateStatement {
    let mut eligible = Query::select();
    eligible
        .column(TemporalAppendIntents::Sequence)
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::AvailableAt).lte(Expr::current_timestamp()))
        .and_where(
            Expr::col(TemporalAppendIntents::LeaseUntil)
                .is_null()
                .or(Expr::col(TemporalAppendIntents::LeaseUntil).lte(Expr::current_timestamp())),
        );
    if let Some(entity_id) = entity_id {
        eligible.and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id));
    }
    eligible
        .order_by(TemporalAppendIntents::Sequence, Order::Asc)
        .limit(<i64 as Ord>::max(limit, 0) as u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);

    Query::update()
        .table(TemporalAppendIntents::Table)
        .value(TemporalAppendIntents::LeaseOwner, owner)
        .value(TemporalAppendIntents::LeaseUntil, lease_until)
        .value(TemporalAppendIntents::Attempts, Expr::col(TemporalAppendIntents::Attempts).add(1))
        .and_where(Expr::col(TemporalAppendIntents::Sequence).in_subquery(eligible.to_owned()))
        .returning(Query::returning().columns([TemporalAppendIntents::Sequence, TemporalAppendIntents::Intent, TemporalAppendIntents::Attempts]))
        .to_owned()
}

/// Read pending intents in sequence order, optionally for one entity.
fn pending_statement(entity_id: Option<&str>) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column(TemporalAppendIntents::Sequence)
        .column(TemporalAppendIntents::Intent)
        .from(TemporalAppendIntents::Table);
    if let Some(entity_id) = entity_id {
        statement.and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id));
    }
    statement.order_by(TemporalAppendIntents::Sequence, Order::Asc).to_owned()
}

/// Remove one acknowledged intent held under this worker's lease.
fn acknowledge_statement(sequence: i64, write_id: Uuid, owner: Uuid) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::Sequence).eq(sequence))
        .and_where(Expr::col(TemporalAppendIntents::WriteId).eq(write_id))
        .and_where(Expr::col(TemporalAppendIntents::LeaseOwner).eq(owner))
        .to_owned()
}

/// Reschedule one intent held under this worker's lease and release the lease.
fn retry_statement(available_at: DateTime<Utc>, last_error: &str, sequence: i64, write_id: Uuid, owner: Uuid) -> UpdateStatement {
    Query::update()
        .table(TemporalAppendIntents::Table)
        .value(TemporalAppendIntents::AvailableAt, available_at)
        .value(TemporalAppendIntents::LastError, last_error)
        .value(TemporalAppendIntents::LeaseOwner, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(Expr::col(TemporalAppendIntents::Sequence).eq(sequence))
        .and_where(Expr::col(TemporalAppendIntents::WriteId).eq(write_id))
        .and_where(Expr::col(TemporalAppendIntents::LeaseOwner).eq(owner))
        .to_owned()
}

/// Classify one anchor-store failure at the temporal port boundary.
fn persistence(error: impl std::fmt::Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::intent_store::{acknowledge_statement, claim_statement, enqueue_statement, intent_unnest_columns, pending_statement, retry_statement};
    use andromeda_postgresql_seaquery::unnest::apply_column_alias;
    use chrono::Utc;
    use sea_query::{Alias, PostgresQueryBuilder};
    use serde_json::json;
    use uuid::Uuid;

    fn enqueue_sql() -> String {
        let statement = enqueue_statement(vec![Uuid::nil()], &["urn:ngsi-ld:Vehicle:1"], vec![0], vec![json!({})]).expect("arity");
        let sql = statement.to_string(PostgresQueryBuilder);
        apply_column_alias(sql, &Alias::new("input"), &intent_unnest_columns())
    }

    #[test]
    fn a_commit_replayed_under_its_original_identity_enqueues_no_second_intent_per_entity_and_base() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "entity_id", "base") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_deduplicating_key_names_the_write_the_entity_and_the_ordinal_base() {
        let sql = enqueue_sql();
        assert!(
            sql.contains(r#"INTO "temporal_append_intents" ("write_id", "entity_id", "base", "intent")"#),
            "{sql}"
        );
    }

    /// The parallel-array set-write projects a `AS input(...)` column-list
    /// alias with no ordinality, and each array is cast to its element type.
    #[test]
    fn the_enqueue_projects_the_four_parallel_arrays_as_named_columns() {
        let sql = enqueue_sql();
        assert!(sql.contains("AS input(write_id, entity_id, base, intent)"), "{sql}");
        assert!(sql.contains("::uuid[]") && sql.contains("::jsonb[]"), "{sql}");
        assert!(!sql.contains("WITH ORDINALITY"), "{sql}");
    }

    /// A batch commit in `BatchSideEffectMode::Enqueue` mode keys on `base` as
    /// well as `(write_id, entity_id)` so two intents for the same write and
    /// entity at different ordinals are not collided.
    #[test]
    fn two_intents_for_the_same_write_and_entity_are_distinguished_by_their_ordinal_base() {
        let sql = enqueue_sql();
        assert!(!sql.contains(r#"ON CONFLICT ("write_id", "entity_id") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_claim_steps_over_locked_rows_and_leases_the_batch() {
        let sql = claim_statement(Uuid::nil(), None, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "temporal_append_intents""#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains(r#""attempts" = "attempts" + 1"#), "{sql}");
    }

    #[test]
    fn a_claim_for_one_entity_restricts_the_eligible_set() {
        let sql = claim_statement(Uuid::nil(), Some("urn:ngsi-ld:Vehicle:1"), 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
    }

    #[test]
    fn pending_reads_are_ordered_by_sequence() {
        let all = pending_statement(None).to_string(PostgresQueryBuilder);
        assert!(all.contains(r#"ORDER BY "sequence""#), "{all}");
        let one = pending_statement(Some("urn:ngsi-ld:Vehicle:1")).to_string(PostgresQueryBuilder);
        assert!(one.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{one}");
    }

    #[test]
    fn acknowledgement_and_retry_are_guarded_by_lease_ownership() {
        let ack = acknowledge_statement(1, Uuid::nil(), Uuid::nil()).to_string(PostgresQueryBuilder);
        assert!(
            ack.contains(r#"DELETE FROM "temporal_append_intents""#) && ack.contains(r#""lease_owner" ="#),
            "{ack}"
        );
        let retry = retry_statement(Utc::now(), "boom", 1, Uuid::nil(), Uuid::nil()).to_string(PostgresQueryBuilder);
        assert!(
            retry.contains(r#""lease_owner" = NULL"#) && retry.contains(r#""lease_until" = NULL"#),
            "{retry}"
        );
    }
}
