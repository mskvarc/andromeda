//! Durable delivery queue for Snapshot lifecycle notifications.

use crate::{
    json_text::JsonText,
    snapshot::{json_value::optional_json, mapper::row_to_snapshot, persistence_error, record::snapshot_select},
    store_fault::undecodable,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, snapshot::SnapshotStatus};
use andromeda_postgresql_schema::snapshot::{SnapshotNotificationOutbox, Snapshots};
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, queue::claimable_at};
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, InsertStatement, JoinType, Keyword, LockBehavior, LockType, Order, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use serde::Deserialize;
use serde_json::value::RawValue;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
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
    /// The identifier the written notification names itself by.
    pub notification_id: Identifier,
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

    /// Claim a due batch using a renewable, replica-safe lease.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the claim, the projection of
    /// the claimed rows or the commit fails, and when a claimed row does not map
    /// back to a delivery because its endpoint or Snapshot URI no longer parses.
    pub async fn claim(&self, worker: LeaseHolder, lease_until: DateTime<Utc>, limit: i32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError> {
        let mut transaction = self.pool.begin().await.map_err(|error| persistence_error(error))?;
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
            // A batch of at most zero rows would spin the worker, so the floor is
            // one row rather than the caller's number.
            .limit(Ord::max(u64::try_from(limit).unwrap_or(1), 1))
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .to_owned();
        let claim = Query::update()
            .table(SnapshotNotificationOutbox::Table)
            .value(SnapshotNotificationOutbox::ClaimedAt, now())
            .value(SnapshotNotificationOutbox::ClaimedBy, worker.as_uuid())
            .value(SnapshotNotificationOutbox::LeaseUntil, lease_until)
            .and_where(Expr::col(SnapshotNotificationOutbox::Id).in_subquery(candidates))
            .returning_col(SnapshotNotificationOutbox::Id)
            .to_owned();
        let (sql, values) = claim.build_sqlx(PostgresQueryBuilder);
        let claimed_ids: Vec<Uuid> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| persistence_error(error))?;
        if claimed_ids.is_empty() {
            transaction.commit().await.map_err(|error| persistence_error(error))?;
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
        let (sql, values) = projection.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| persistence_error(error))?;
        transaction.commit().await.map_err(|error| persistence_error(error))?;
        rows.iter().map(map_delivery).collect()
    }

    /// Mark a leased event delivered.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::persistence`] when the update fails.
    pub async fn acknowledge(&self, id: Uuid, worker: LeaseHolder) -> Result<bool, SnapshotError> {
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
        Ok(execute(&self.pool, statement).await? == 1)
    }

    /// Record a failed attempt, scheduling retry or dead-lettering it.
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
        retry_at: DateTime<Utc>,
        detail: &str,
    ) -> Result<bool, SnapshotError> {
        let max_attempts = <i32 as Ord>::max(max_attempts, 1);
        let retry_due =
            || Expr::val(failure == SnapshotDeliveryFailure::Retryable).and(Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1).lt(max_attempts));
        let exhausted =
            || Expr::val(failure == SnapshotDeliveryFailure::Terminal).or(Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1).gte(max_attempts));
        let next_attempt = CaseStatement::new()
            .case(retry_due(), Expr::val(retry_at))
            .finally(Expr::col(SnapshotNotificationOutbox::NextAttemptAt));
        let exhausted_at = CaseStatement::new().case(exhausted(), now()).finally(Keyword::Null);
        let statement = Query::update()
            .table(SnapshotNotificationOutbox::Table)
            .value(
                SnapshotNotificationOutbox::AttemptCount,
                Expr::col(SnapshotNotificationOutbox::AttemptCount).add(1),
            )
            .value(SnapshotNotificationOutbox::NextAttemptAt, next_attempt)
            .value(SnapshotNotificationOutbox::ExhaustedAt, exhausted_at)
            .value(SnapshotNotificationOutbox::LastError, detail)
            .value(SnapshotNotificationOutbox::ClaimedAt, Keyword::Null)
            .value(SnapshotNotificationOutbox::ClaimedBy, Keyword::Null)
            .value(SnapshotNotificationOutbox::LeaseUntil, Keyword::Null)
            .and_where(Expr::col(SnapshotNotificationOutbox::Id).eq(id))
            .and_where(Expr::col(SnapshotNotificationOutbox::ClaimedBy).eq(worker.as_uuid()))
            .and_where(Expr::col(SnapshotNotificationOutbox::DeliveredAt).is_null())
            .and_where(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
            .to_owned();
        Ok(execute(&self.pool, statement).await? == 1)
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
        execute(&self.pool, statement).await
    }
}

/// Queue the current terminal representation inside its lifecycle transaction.
pub(crate) async fn enqueue_terminal_notification(transaction: &mut Transaction<'_, Postgres>, snapshot_id: &Identifier) -> Result<(), SnapshotError> {
    let mut statement = snapshot_select();
    statement
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(snapshot_id.as_str()));
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    let snapshot = row_to_snapshot(&row).map_err(|error| persistence_error(error))?;
    let Some(endpoint) = snapshot.endpoint.as_ref() else {
        return Ok(());
    };
    if snapshot.status == SnapshotStatus::Preparing {
        return Ok(());
    }
    let snapshot_key: i64 = row.try_get("snapshot_key").map_err(|error| persistence_error(error))?;
    let payload = serde_json::to_string(&snapshot.build_notification())?;
    let insert = outbox_insert(snapshot_key, endpoint, snapshot.receiver_info.as_deref(), payload)?;
    let (sql, values) = insert.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error(error))?;
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
            optional_json(receiver_info)?,
            cast(Expr::val(payload), PostgresType::Json),
        ])
        .map_err(|error| persistence_error(error))?;
    Ok(insert)
}

/// Execute one generated outbox mutation and return its affected-row count.
async fn execute(pool: &PgPool, statement: impl SqlxBinder) -> Result<u64, SnapshotError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(result.rows_affected())
}

/// The member of a written notification its delivery is identified by.
#[derive(Deserialize)]
struct NotificationIdentity {
    /// The notification's own identifier.
    id: Identifier,
}

/// Decode one claimed outbox row into its transport-facing delivery value.
fn map_delivery(row: &PgRow) -> Result<SnapshotStatusDelivery, SnapshotError> {
    let raw_snapshot: String = row.try_get("snapshot_id").map_err(|error| persistence_error(error))?;
    let raw_endpoint: String = row.try_get("endpoint").map_err(|error| persistence_error(error))?;
    let payload: JsonText<'_> = row.try_get("payload").map_err(|error| persistence_error(error))?;
    let notification: Box<RawValue> = serde_json::from_slice(payload.as_bytes())?;
    let receiver_info = row
        .try_get::<Option<JsonText<'_>>, _>("receiver_info")
        .map_err(|error| persistence_error(error))?
        .map(|text| serde_json::from_slice(text.as_bytes()))
        .transpose()?;
    Ok(SnapshotStatusDelivery {
        id: row.try_get("id").map_err(|error| persistence_error(error))?,
        snapshot_id: raw_snapshot.parse().map_err(|error| SnapshotError::from(undecodable(&error)))?,
        endpoint: Url::parse(&raw_endpoint).map_err(|error| SnapshotError::from(undecodable(&error)))?,
        receiver_info,
        notification_id: serde_json::from_str::<NotificationIdentity>(notification.get())?.id,
        notification,
        attempt_count: row.try_get("attempt_count").map_err(|error| persistence_error(error))?,
    })
}

#[cfg(test)]
mod tests {
    use crate::snapshot::status_outbox::outbox_insert;
    use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use url::Url;

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
}
