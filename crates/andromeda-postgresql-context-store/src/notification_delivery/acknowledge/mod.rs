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
//! retried failure takes it, a delivery releases it and an exhausted probe
//! passes it to the earliest change still queued, while a deferral leaves it
//! where it is. The settlements of one group were in flight together, so no
//! order between them is owed; the deliveries are recorded first, then the
//! failures, then the exhaustions, then the deferrals, and each sees the holds
//! as the kinds before it left them, as it would had it arrived after them.
//! The probe is always the held Subscription's queued delivery first in change
//! order, so neither that order nor the rounds a Subscription's failures land
//! in decide which delivery probes.
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
        acknowledge::statements::{DeferredRows, ExhaustedRows, FailedRows, deferred_statement, delivered_statement, exhausted_statement, failed_statement},
        hold::{hand_over::hand_over_statement, release::resume_statement, take::set_aside_statement},
    },
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::{
    delivery_backoff::BackoffStep,
    delivery_settlement::{DeliverySettlement, DeliveryVerdict},
};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
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
    /// How long after the acknowledgement the next probe of each exhausted
    /// delivery's Subscription is due, if it was the probe.
    next_probes: HashMap<i64, BackoffStep>,
    /// The deferred deliveries.
    deferred: DeferredRows,
}

impl<'a> BranchSettlements<'a> {
    /// Add one settlement to its kind.
    fn add(&mut self, key: i64, verdict: &'a DeliveryVerdict) {
        match verdict {
            DeliveryVerdict::Delivered => self.delivered.push(key),
            DeliveryVerdict::Failed { detail, retry_after } => {
                self.failed.delivery_keys.push(key);
                self.failed.failures.push(detail);
                self.failed.retry_steps.push(*retry_after);
            }
            DeliveryVerdict::Exhausted { detail, probe_after } => {
                self.exhausted.delivery_keys.push(key);
                self.exhausted.failures.push(detail);
                self.next_probes.insert(key, *probe_after);
            }
            DeliveryVerdict::Deferred { retry_after } => {
                self.deferred.delivery_keys.push(key);
                self.deferred.retry_steps.push(*retry_after);
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
        let keys = record_deferred(connection, target, worker_id, branch.deferred).await?;
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

/// Record the failed attempts of `rows` and set aside each held
/// Subscription's queue but its probe; returns the keys recorded.
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
    let recorded: Vec<(i64, i64)> = observe_statement(
        ObservedStatement::NotificationDeliveryAcknowledge,
        statement.to_query_as().fetch_all(&mut *connection),
    )
    .await?;
    let held: BTreeSet<i64> = recorded.iter().map(|(_, subscription_key)| *subscription_key).collect();
    for subscription_key in held {
        observe_statement(
            ObservedStatement::NotificationDeliveryHold,
            set_aside_statement(target, subscription_key).to_query().execute(&mut *connection),
        )
        .await?;
    }
    Ok(recorded.into_iter().map(|(key, _)| key).collect())
}

/// Record the exhausted deliveries of `rows` and pass each hold one of them
/// was the probe of to the Subscription's queued delivery first in change
/// order, due the step `next_probes` names for that probe after the
/// transaction began; returns the keys recorded.
async fn record_exhausted(
    connection: &mut PgConnection,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    rows: ExhaustedRows<'_>,
    next_probes: &HashMap<i64, BackoffStep>,
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
        if let (Some(subscription_key), Some(probe_after)) = (subscription_key, next_probes.get(probe)) {
            observe_statement(
                ObservedStatement::NotificationDeliveryHold,
                hand_over_statement(target, *subscription_key, *probe_after)
                    .to_query()
                    .execute(&mut *connection),
            )
            .await?;
        }
    }
    Ok(recorded.into_iter().map(|(key, _)| key).collect())
}

/// Record the deferred deliveries of `rows`, each due its own step after the
/// transaction began; returns the keys recorded.
async fn record_deferred(
    connection: &mut PgConnection,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    rows: DeferredRows,
) -> Result<Vec<i64>, ContextStoreError> {
    if rows.delivery_keys.is_empty() {
        return Ok(Vec::new());
    }
    Ok(observe_statement(
        ObservedStatement::NotificationDeliveryAcknowledge,
        deferred_statement(target, worker_id, rows).to_query_scalar::<i64>().fetch_all(&mut *connection),
    )
    .await?)
}
