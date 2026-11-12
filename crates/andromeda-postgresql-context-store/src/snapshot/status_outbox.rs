//! Durable delivery queue for Snapshot lifecycle notifications.
//!
//! Every lease and retry instant is set on the database clock, the clock the
//! claim compares it with, so a broker whose clock differs from the database's
//! neither stretches a lease nor moves a retry.

use crate::{
    error::ContextStoreError,
    json_text::JsonText,
    snapshot::{json_value::optional_json, mapper::row_to_snapshot, record::snapshot_select, unencodable_snapshot_error::UnencodableSnapshotError},
    store_fault::{persistence_error, undecodable},
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, snapshot::SnapshotStatus};
use andromeda_notification::delivery_backoff::BackoffStep;
use andromeda_postgresql_schema::snapshot::{SnapshotNotificationOutbox, Snapshots};
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::{now, span_after_transaction_start},
    pg_type::PostgresType,
    queue::{claimable_at, is_claimed_from},
    statement::Statement,
};
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, InsertStatement, JoinType, Keyword, LockBehavior, LockType, Order, Query, UpdateStatement};
use serde_json::value::RawValue;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{num::NonZeroU32, time::Duration};
use url::Url;
use uuid::Uuid;

/// One leased Snapshot status notification ready for transport.
#[derive(Debug)]
pub struct SnapshotStatusDelivery {
    /// Durable outbox identity.
    pub id: Uuid,
    /// Snapshot identifier used for correlation and transport logging.
    pub snapshot_id: Identifier,
    /// HTTP callback endpoint captured when the lifecycle event committed.
    pub endpoint: Url,
    /// Receiver-supplied HTTP headers captured with the event.
    pub receiver_info: Option<Vec<KeyValuePair>>,
    /// The `SnapshotNotification` (clause 5.3.4) as it was written when the
    /// lifecycle event committed, `@context` included.
    pub notification: Box<RawValue>,
    /// Number of attempts already completed before this lease.
    pub attempt_count: i32,
}

/// Whether a failed status delivery may be retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotDeliveryFailure {
    /// Retry the delivery while its attempt budget remains.
    Retryable,
    /// Dead-letter the delivery immediately.
    Terminal,
}

/// PostgreSQL-backed Snapshot status notification outbox.
pub struct SnapshotStatusOutbox {
    /// Tenant database containing the durable status-delivery queue.
    pool: PgPool,
}

impl SnapshotStatusOutbox {
    /// Construct an outbox over one tenant database.
    #[must_use]
    pub fn new(pool: &PgPool) -> SnapshotStatusOutbox {
        SnapshotStatusOutbox { pool: pool.clone() }
    }

    /// Claim a due batch, each delivery leased for `lease` from the claim on
    /// the database clock.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the claim, the projection of
    /// the claimed rows or the commit fails, and when a claimed row does not map
    /// back to a delivery because its endpoint or Snapshot URI no longer parses.
    pub async fn claim(&self, worker: LeaseHolder, lease: Duration, limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError> {
        let mut transaction = self.pool.begin().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
        let claimed_ids: Vec<Uuid> = claim_statement(worker, lease, limit)
            .to_query_scalar()
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        if claimed_ids.is_empty() {
            transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
            return Ok(Vec::new());
        }

        let projection = Query::select()
            .columns([
                (SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::Id),
                (SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::Endpoint),
                (SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::ReceiverInfo),
                (SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::Payload),
                (SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::AttemptCount),
            ])
            .expr_as(Expr::col((Snapshots::Table, Snapshots::Id)), Alias::new("snapshot_id"))
            .from(SnapshotNotificationOutbox::Table)
            .join(
                JoinType::InnerJoin,
                Snapshots::Table,
                Expr::col((Snapshots::Table, Snapshots::SnapshotKey))
                    .eq(Expr::col((SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::SnapshotKey))),
            )
            .and_where(Expr::col((SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::Id)).is_in(claimed_ids))
            .order_by((SnapshotNotificationOutbox::Table, SnapshotNotificationOutbox::Id), Order::Asc)
            .to_owned();
        let rows = projection
            .to_query()
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
        rows.iter().map(map_delivery).collect()
    }

    /// Extend the lease a worker holds on one claimed event to `lease` from
    /// the renewal.
    ///
    /// The guard is the one every acknowledgement carries: the row must still be
    /// claimed by `worker` and neither delivered nor dead-lettered, so a row
    /// another worker reclaimed after its lease lapsed is never extended back.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the update fails.
    pub async fn renew(&self, id: Uuid, worker: LeaseHolder, lease: Duration) -> Result<LeaseAcknowledgement, SnapshotError> {
        let result = renew_statement(id, worker, lease)
            .to_query()
            .execute(&self.pool)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }

    /// Mark a leased event delivered, reporting whether the worker still held it.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the update fails.
    pub async fn acknowledge(&self, id: Uuid, worker: LeaseHolder) -> Result<LeaseAcknowledgement, SnapshotError> {
        let statement = Query::update()
            .table(SnapshotNotificationOutbox::Table)
            .value(SnapshotNotificationOutbox::DeliveredAt, now())
            .value(
                SnapshotNotificationOutbox::AttemptCount,
                Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1),
            )
            .and_where(Expr::col(SnapshotNotificationOutbox::Id).eq(id))
            .and_where(Expr::col(SnapshotNotificationOutbox::ClaimedBy).eq(worker.as_uuid()))
            .and_where(Expr::col(SnapshotNotificationOutbox::DeliveredAt).is_null())
            .and_where(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
            .to_owned();
        let result = statement
            .to_query()
            .execute(&self.pool)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }

    /// Record a failed attempt, scheduling its retry `retry_after` from the
    /// failure on the database clock or dead-lettering it, and report whether
    /// the worker still held the event.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the update fails.
    pub async fn fail(
        &self,
        id: Uuid,
        worker: LeaseHolder,
        failure: SnapshotDeliveryFailure,
        max_attempts: i32,
        retry_after: BackoffStep,
        detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError> {
        let result = fail_statement(id, worker, failure, max_attempts, retry_after, detail)
            .to_query()
            .execute(&self.pool)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }

    /// Remove old delivered and dead-lettered rows.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the delete fails.
    pub async fn cleanup(&self, delivered_before: DateTime<Utc>, exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError> {
        let statement = Query::delete()
            .from_table(SnapshotNotificationOutbox::Table)
            .cond_where(
                Expr::col(SnapshotNotificationOutbox::DeliveredAt)
                    .lte(delivered_before)
                    .or(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).lte(exhausted_before)),
            )
            .to_owned();
        let result = statement
            .to_query()
            .execute(&self.pool)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?;
        Ok(result.rows_affected())
    }
}

/// Lease up to `limit` due events to `worker` for `lease` from the claim,
/// returning their ids.
///
/// The update leases exactly the ids its `FOR UPDATE SKIP LOCKED` selection
/// picked ([`is_claimed_from`]): an event leased past the batch would wait out
/// its lease undelivered.
fn claim_statement(worker: LeaseHolder, lease: Duration, limit: NonZeroU32) -> UpdateStatement {
    let claimable = || {
        claimable_at(
            Expr::col(SnapshotNotificationOutbox::NextAttemptAt),
            Expr::col(SnapshotNotificationOutbox::LeaseUntil),
        )
    };
    let candidates = Query::select()
        .column(SnapshotNotificationOutbox::Id)
        .from(SnapshotNotificationOutbox::Table)
        .and_where(Expr::col(SnapshotNotificationOutbox::DeliveredAt).is_null())
        .and_where(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
        .and_where(claimable().lte(Expr::current_timestamp()))
        .order_by_expr(claimable(), Order::Asc)
        .order_by(SnapshotNotificationOutbox::Id, Order::Asc)
        .limit(u64::from(limit.get()))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();
    Query::update()
        .table(SnapshotNotificationOutbox::Table)
        .value(SnapshotNotificationOutbox::ClaimedBy, worker.as_uuid())
        .value(SnapshotNotificationOutbox::LeaseUntil, span_after_transaction_start(lease))
        .and_where(is_claimed_from(Expr::col(SnapshotNotificationOutbox::Id), candidates))
        .returning_col(SnapshotNotificationOutbox::Id)
        .to_owned()
}

/// Count a failed attempt on one claimed event and release its lease: due
/// again `retry_after` from the failure on the database clock while it has
/// attempts left and the failure is retryable, dead-lettered otherwise.
///
/// Guarded like the acknowledgement: the event must still be claimed by
/// `worker` and neither delivered nor dead-lettered.
fn fail_statement(
    id: Uuid,
    worker: LeaseHolder,
    failure: SnapshotDeliveryFailure,
    max_attempts: i32,
    retry_after: BackoffStep,
    detail: &str,
) -> UpdateStatement {
    let max_attempts = <i32 as Ord>::max(max_attempts, 1);
    let retry_due =
        || Expr::val(failure == SnapshotDeliveryFailure::Retryable).and(Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1).lt(max_attempts));
    let exhausted = || Expr::val(failure == SnapshotDeliveryFailure::Terminal).or(Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1).gte(max_attempts));
    let next_attempt = CaseStatement::new()
        .case(retry_due(), span_after_transaction_start(retry_after.duration()))
        .finally(Expr::col(SnapshotNotificationOutbox::NextAttemptAt));
    let exhausted_at = CaseStatement::new().case(exhausted(), now()).finally(Keyword::Null);
    Query::update()
        .table(SnapshotNotificationOutbox::Table)
        .value(
            SnapshotNotificationOutbox::AttemptCount,
            Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1),
        )
        .value(SnapshotNotificationOutbox::NextAttemptAt, next_attempt)
        .value(SnapshotNotificationOutbox::ExhaustedAt, exhausted_at)
        .value(SnapshotNotificationOutbox::LastError, detail)
        .value(SnapshotNotificationOutbox::ClaimedBy, Keyword::Null)
        .value(SnapshotNotificationOutbox::LeaseUntil, Keyword::Null)
        .and_where(Expr::col(SnapshotNotificationOutbox::Id).eq(id))
        .and_where(Expr::col(SnapshotNotificationOutbox::ClaimedBy).eq(worker.as_uuid()))
        .and_where(Expr::col(SnapshotNotificationOutbox::DeliveredAt).is_null())
        .and_where(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
        .to_owned()
}

/// The lease extension of one claimed event to `lease` from the renewal,
/// guarded like its acknowledgement.
fn renew_statement(id: Uuid, worker: LeaseHolder, lease: Duration) -> UpdateStatement {
    Query::update()
        .table(SnapshotNotificationOutbox::Table)
        .value(SnapshotNotificationOutbox::LeaseUntil, span_after_transaction_start(lease))
        .and_where(Expr::col(SnapshotNotificationOutbox::Id).eq(id))
        .and_where(Expr::col(SnapshotNotificationOutbox::ClaimedBy).eq(worker.as_uuid()))
        .and_where(Expr::col(SnapshotNotificationOutbox::DeliveredAt).is_null())
        .and_where(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
        .to_owned()
}

/// Queue the current terminal representation inside its lifecycle transaction.
pub(crate) async fn enqueue_terminal_notification(transaction: &mut Transaction<'_, Postgres>, snapshot_id: &Identifier) -> Result<(), SnapshotError> {
    let mut statement = snapshot_select();
    statement
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(snapshot_id.as_str()));
    let row = statement
        .to_query()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let snapshot = row_to_snapshot(&row).map_err(|error| persistence_error::<SnapshotError>(error))?;
    let Some(endpoint) = snapshot.endpoint.as_ref() else {
        return Ok(());
    };
    if snapshot.status == SnapshotStatus::Preparing {
        return Ok(());
    }
    let snapshot_key: i64 = row.try_get("snapshot_key").map_err(|error| persistence_error::<SnapshotError>(error))?;
    let payload = serde_json::to_string(&snapshot.build_notification())
        .map_err(|error| persistence_error::<SnapshotError>(ContextStoreError::from(UnencodableSnapshotError::Notification(error))))?;
    let insert = outbox_insert(snapshot_key, endpoint, snapshot.receiver_info.as_deref(), payload)?;
    insert
        .to_query()
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

/// The outbox row of one terminal notification: its receiver info and its
/// payload are `json` columns, bound as their JSON text cast to `json`.
fn outbox_insert(snapshot_key: i64, endpoint: &Url, receiver_info: Option<&[KeyValuePair]>, payload: String) -> Result<InsertStatement, SnapshotError> {
    let mut insert = Query::insert();
    insert
        .into_table(SnapshotNotificationOutbox::Table)
        .columns([
            SnapshotNotificationOutbox::Id,
            SnapshotNotificationOutbox::SnapshotKey,
            SnapshotNotificationOutbox::Endpoint,
            SnapshotNotificationOutbox::ReceiverInfo,
            SnapshotNotificationOutbox::Payload,
        ])
        .values([
            Expr::val(Uuid::new_v4()),
            Expr::val(snapshot_key),
            Expr::val(endpoint.as_str()),
            optional_json(receiver_info, UnencodableSnapshotError::ReceiverInfo)?,
            cast(Expr::val(payload), PostgresType::Json),
        ])
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(insert)
}

/// Decode one claimed outbox row into its transport-facing delivery value.
fn map_delivery(row: &PgRow) -> Result<SnapshotStatusDelivery, SnapshotError> {
    let raw_snapshot: String = row.try_get("snapshot_id").map_err(|error| persistence_error::<SnapshotError>(error))?;
    let raw_endpoint: String = row.try_get("endpoint").map_err(|error| persistence_error::<SnapshotError>(error))?;
    let payload: JsonText<'_> = row.try_get("payload").map_err(|error| persistence_error::<SnapshotError>(error))?;
    let notification: Box<RawValue> = serde_json::from_slice(payload.as_bytes())?;
    let receiver_info = row
        .try_get::<Option<JsonText<'_>>, _>("receiver_info")
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .map(|text| serde_json::from_slice(text.as_bytes()))
        .transpose()?;
    Ok(SnapshotStatusDelivery {
        id: row.try_get("id").map_err(|error| persistence_error::<SnapshotError>(error))?,
        snapshot_id: raw_snapshot.parse().map_err(|error| SnapshotError::from(undecodable(&error)))?,
        endpoint: Url::parse(&raw_endpoint).map_err(|error| SnapshotError::from(undecodable(&error)))?,
        receiver_info,
        notification,
        attempt_count: row.try_get("attempt_count").map_err(|error| persistence_error::<SnapshotError>(error))?,
    })
}

#[cfg(test)]
mod tests {
    use crate::snapshot::status_outbox::{SnapshotDeliveryFailure, claim_statement, fail_statement, outbox_insert, renew_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use andromeda_notification::delivery_backoff::{BackoffStep, DeliveryBackoff};
    use andromeda_retry::schedule::BackoffSchedule;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::{num::NonZeroU32, time::Duration};
    use url::Url;
    use uuid::Uuid;

    const LEASE: Duration = Duration::from_secs(30);

    fn five_seconds() -> BackoffStep {
        DeliveryBackoff::new(BackoffSchedule::constant(Duration::from_secs(5)))
            .expect("a schedule within the longest step")
            .first_step()
    }

    #[test]
    fn a_lease_renewal_reaches_only_an_undelivered_event_the_worker_still_claims() {
        let worker = LeaseHolder::from(Uuid::nil());
        let sql = renew_statement(Uuid::nil(), worker, LEASE).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"SET "lease_until" = CURRENT_TIMESTAMP + (make_interval(secs => 30))"#), "{sql}");
        assert!(sql.contains(r#""id" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(sql.contains(r#""delivered_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn the_outbox_payload_and_receiver_info_are_bound_as_json() {
        let endpoint = Url::parse("http://receiver.example/notify").expect("a valid URL");
        let receiver_info = [KeyValuePair {
            key: "fiware-service".into(),
            value: "openiot".into(),
        }];
        let sql = outbox_insert(7, &endpoint, Some(&receiver_info), r#"{"id":"urn:ngsi-ld:Notification:1"}"#.to_owned())
            .expect("the outbox insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches("::json").count(), 2, "{sql}");
        assert!(sql.contains(r#"'[{"key":"fiware-service","value":"openiot"}]'::json"#), "{sql}");
        assert!(sql.contains(r#"'{"id":"urn:ngsi-ld:Notification:1"}'::json"#), "{sql}");
        assert!(!sql.contains("::jsonb"), "{sql}");
    }

    /// The claim compares a lapsed lease with the database clock, so the claim
    /// and every renewal set the lease on that clock: a lease from a broker
    /// running ahead would hold a stopped worker's deliveries back that much
    /// longer.
    #[test]
    fn the_claim_leases_each_event_for_the_lease_span_after_the_database_clock() {
        let limit = NonZeroU32::new(16).expect("a positive limit");
        let sql = claim_statement(LeaseHolder::from(Uuid::nil()), LEASE, limit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""lease_until" = CURRENT_TIMESTAMP + (make_interval(secs => 30))"#), "{sql}");
    }

    /// The claim compares `next_attempt_at` with the database clock, so a
    /// retryable failure is due one step after that clock; a failure that ends
    /// the attempts keeps the instant it had.
    #[test]
    fn a_retryable_failure_is_due_one_step_after_the_database_clock() {
        let sql = fail_statement(
            Uuid::nil(),
            LeaseHolder::from(Uuid::nil()),
            SnapshotDeliveryFailure::Retryable,
            5,
            five_seconds(),
            "boom",
        )
        .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(concat!(
                r#""next_attempt_at" = (CASE WHEN (TRUE AND "attempt_count" + 1 < 5) "#,
                r#"THEN CURRENT_TIMESTAMP + (make_interval(secs => 5)) ELSE "next_attempt_at" END)"#
            )),
            "{sql}"
        );
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
    }

    #[test]
    fn the_claim_leases_only_the_ids_its_limited_selection_picked() {
        let limit = NonZeroU32::new(16).expect("a positive limit");
        let sql = claim_statement(LeaseHolder::from(Uuid::nil()), LEASE, limit).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"UPDATE "snapshot_notification_outbox" SET"#), "{sql}");
        assert!(
            sql.contains(r#"WHERE "id" = ANY(ARRAY (SELECT "id" FROM "snapshot_notification_outbox""#),
            "{sql}"
        );
        assert!(sql.contains(r#""id" ASC LIMIT 16 FOR UPDATE SKIP LOCKED))"#), "{sql}");
        assert!(!sql.contains(" IN (SELECT"), "{sql}");
        assert!(sql.ends_with(r#"RETURNING "id""#), "{sql}");
    }
}
