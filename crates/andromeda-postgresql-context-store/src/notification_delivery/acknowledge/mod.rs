//! Releasing claimed deliveries back to the queue, or out of it, a group at a
//! time.
//!
//! A worker hands over the settlements of the deliveries whose attempts ended
//! while its previous round was in flight, and the whole group is recorded in
//! the round's transaction: per branch, one set-based statement for each kind
//! of settlement the group holds, then the hold changes those statements call
//! for. A group that fails rolls the round back whole, so the worker can ask
//! again without recording any delivery twice.
//!
//! Every statement is guarded by the claim each row was taken under (see
//! [`statements`]): a row this worker no longer leases is left as the worker
//! that reclaimed it has it, and its settlement is answered
//! [`LeaseAcknowledgement::Lost`] rather than failing the group.
//!
//! Each settlement also moves the hold of its delivery's Subscription
//! ([`crate::notification_delivery::hold`]) in the same transaction: a
//! retryable failure takes it, a delivery releases it and an exhausted probe
//! hands it over. The settlements of one group were in flight together, so
//! no order between them is owed; the deliveries are recorded first, then the
//! failures, then the exhaustions, and each sees the holds as the kinds
//! before it left them, as it would had it arrived after them.
//!
//! The failure diagnostic arrives already trimmed to the length the schema
//! accepts, because the bound is a delivery-policy decision the notification
//! crate owns. Nothing here shortens it a second time: what is stored is
//! exactly what the worker recorded.

/// The set-based statements recording one kind of settlement.
mod statements;

use crate::{
    error::ContextStoreError,
    notification_delivery::{
        acknowledge::statements::{ExhaustedRows, FailedRows, delivered_statement, exhausted_statement, failed_statement},
        hold::{HeldSubscription, hand_over_statement, resume_statement, set_aside_statement},
    },
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::delivery_settlement::{DeliverySettlement, DeliveryVerdict};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use sqlx::postgres::PgConnection;
use std::collections::{BTreeSet, HashMap, HashSet};

/// The settlements of one group on one branch, by kind.
#[derive(Debug, Default)]
struct BranchSettlements<'a> {
    /// The deliveries delivered.
    delivered: Vec<i64>,
    /// The failed attempts.
    failed: FailedRows<'a>,
    /// The exhausted deliveries.
    exhausted: ExhaustedRows<'a>,
    /// When the next probe of each exhausted delivery's Subscription is due,
    /// if it was the probe.
    next_probes: HashMap<i64, DateTime<Utc>>,
}

impl<'a> BranchSettlements<'a> {
    /// Add one settlement to its kind.
    fn add(&mut self, key: i64, verdict: &'a DeliveryVerdict) {
        match verdict {
            DeliveryVerdict::Delivered => self.delivered.push(key),
            DeliveryVerdict::Failed { detail, next_attempt_at } => {
                self.failed.delivery_keys.push(key);
                self.failed.failures.push(detail);
                self.failed.next_attempts.push(*next_attempt_at);
            }
            DeliveryVerdict::Exhausted { detail, next_probe_at } => {
                self.exhausted.delivery_keys.push(key);
                self.exhausted.failures.push(detail);
                self.next_probes.insert(key, *next_probe_at);
            }
        }
    }
}

/// Record every settlement of `settlements` that `worker_id` still holds the
/// lease of, on `connection` inside the caller's transaction, and move the
/// holds of their Subscriptions.
///
/// Returns one answer per settlement, in order.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails, and
/// [`ContextStoreError::StatementBuild`] when one cannot be built; the
/// transaction is then left for the caller to roll back.
pub(crate) async fn record_settlements(
    connection: &mut PgConnection,
    worker_id: LeaseHolder,
    settlements: &[DeliverySettlement],
) -> Result<Vec<LeaseAcknowledgement>, ContextStoreError> {
    let mut branches: IndexMap<&ContextTarget, BranchSettlements<'_>> = IndexMap::new();
    for settlement in settlements {
        branches
            .entry(&settlement.target)
            .or_default()
            .add(settlement.key.into_inner(), &settlement.verdict);
    }
    let mut recorded: HashSet<(&ContextTarget, i64)> = HashSet::new();
    for (target, branch) in branches {
        let next_probes = branch.next_probes;
        let keys = record_delivered(connection, target, worker_id, branch.delivered).await?;
        recorded.extend(keys.into_iter().map(|key| (target, key)));
        let keys = record_failed(connection, target, worker_id, branch.failed).await?;
        recorded.extend(keys.into_iter().map(|key| (target, key)));
        let keys = record_exhausted(connection, target, worker_id, branch.exhausted, &next_probes).await?;
        recorded.extend(keys.into_iter().map(|key| (target, key)));
    }
    Ok(settlements
        .iter()
        .map(|settlement| {
            if recorded.contains(&(&settlement.target, settlement.key.into_inner())) {
                LeaseAcknowledgement::Applied
            } else {
                LeaseAcknowledgement::Lost
            }
        })
        .collect())
}

/// Record `delivery_keys` delivered and take back every delivery the holds
/// they release set aside; returns the keys recorded.
async fn record_delivered(
    connection: &mut PgConnection,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    delivery_keys: Vec<i64>,
) -> Result<Vec<i64>, ContextStoreError> {
    if delivery_keys.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(i64, Option<i64>)> = observe_statement(
        ObservedStatement::NotificationDeliveryAcknowledge,
        delivered_statement(target, worker_id, delivery_keys).to_query_as().fetch_all(&mut *connection),
    )
    .await?;
    let released: BTreeSet<i64> = rows.iter().filter_map(|(_, released)| *released).collect();
    if !released.is_empty() {
        observe_statement(
            ObservedStatement::NotificationDeliveryHold,
            resume_statement(target, released.into_iter().collect()).to_query().execute(&mut *connection),
        )
        .await?;
    }
    Ok(rows.into_iter().map(|(key, _)| key).collect())
}

/// Record the failed attempts of `rows` and set the rest of each held
/// Subscription's queue aside; returns the keys recorded.
async fn record_failed(
    connection: &mut PgConnection,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    rows: FailedRows<'_>,
) -> Result<Vec<i64>, ContextStoreError> {
    if rows.delivery_keys.is_empty() {
        return Ok(Vec::new());
    }
    let statement = failed_statement(target, worker_id, rows)?;
    let recorded: Vec<(i64, i64, i64)> = observe_statement(
        ObservedStatement::NotificationDeliveryAcknowledge,
        statement.to_query_as().fetch_all(&mut *connection),
    )
    .await?;
    let held: BTreeSet<(i64, i64)> = recorded.iter().map(|(_, subscription, probe)| (*subscription, *probe)).collect();
    for (subscription_key, probe_delivery_key) in held {
        let held = HeldSubscription {
            subscription_key,
            probe_delivery_key,
        };
        observe_statement(
            ObservedStatement::NotificationDeliveryHold,
            set_aside_statement(target, held).to_query().execute(&mut *connection),
        )
        .await?;
    }
    Ok(recorded.into_iter().map(|(key, _, _)| key).collect())
}

/// Record the exhausted deliveries of `rows` and hand each hold one of them
/// was the probe of to its successor, due at the instant `next_probes` names
/// for that probe; returns the keys recorded.
async fn record_exhausted(
    connection: &mut PgConnection,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    rows: ExhaustedRows<'_>,
    next_probes: &HashMap<i64, DateTime<Utc>>,
) -> Result<Vec<i64>, ContextStoreError> {
    if rows.delivery_keys.is_empty() {
        return Ok(Vec::new());
    }
    let recorded: Vec<(i64, Option<i64>)> = observe_statement(
        ObservedStatement::NotificationDeliveryAcknowledge,
        exhausted_statement(target, worker_id, rows).to_query_as().fetch_all(&mut *connection),
    )
    .await?;
    for (probe, subscription_key) in &recorded {
        if let (Some(subscription_key), Some(next_probe_at)) = (subscription_key, next_probes.get(probe)) {
            observe_statement(
                ObservedStatement::NotificationDeliveryHold,
                hand_over_statement(target, *subscription_key, *next_probe_at)
                    .to_query()
                    .execute(&mut *connection),
            )
            .await?;
        }
    }
    Ok(recorded.into_iter().map(|(key, _)| key).collect())
}
