//! Catching a subscription's record up after registration changes were lost.
//!
//! The registration change channel is bounded and drops its oldest events when
//! a worker falls behind, so the changes the worker never received cannot be
//! evaluated one by one. What they did is still readable: the registrations a
//! subscription selects now, against the registrations it was recorded as
//! notified of.

use crate::{
    csource_match_set::matching_registrations,
    csource_processor::{CSR_NOTIFICATION_CONCURRENCY, CsourceNotificationProcessor},
    error::NotificationError,
};
use andromeda_subscription::csource_matching::csource_subscription_follows_registration_changes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use tracing::error;

impl CsourceNotificationProcessor {
    /// Notify every change-driven subscription of the registration changes it
    /// missed, after the registration change channel dropped events.
    ///
    /// The subscription cache is reloaded first, then each subscription is
    /// brought in line with the registrations as they stand. A registration
    /// it now selects and was not told of is reported as `newlyMatching`, and
    /// one it was told of that it no longer selects as `noLongerMatching`
    /// (clauses 5.11.2.4 and 5.3.3). A registration it was told of and still
    /// selects is reported as `updated` when it was modified after
    /// `evaluated_through`, the modification time of the last change this
    /// worker evaluated. Each report is claimed in the notified record before
    /// it is sent, so a change that was delivered after all, or a second
    /// broker reconciling the same tenant, reports nothing twice.
    ///
    /// Returns the latest modification time among the registrations read, or
    /// `None` when it read none: the point the next reconciliation reports
    /// `updated` changes from, and up to which the changes still queued were
    /// already reported.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store
    /// cannot list the tenant's active subscriptions; nothing is then
    /// reconciled.
    pub async fn reconcile_registrations(&self, evaluated_through: Option<DateTime<Utc>>) -> Result<Option<DateTime<Utc>>, NotificationError> {
        self.reload_cache().await?;
        let mut read_through = None;
        let subscriptions = self
            .cache
            .subscriptions()
            .into_iter()
            .filter(|subscription| csource_subscription_follows_registration_changes(subscription));
        for subscription in subscriptions {
            let matching = match matching_registrations(self.csr_repo.as_ref(), &subscription).await {
                Ok(matching) => matching,
                Err(failure) => {
                    error!("Failed to read the CSRs matching subscription {} to reconcile it: {}", subscription.id, failure);
                    continue;
                }
            };
            let departed = self.recorded_beyond_states(&subscription, &matching).await;
            let modified = matching
                .iter()
                .chain(departed.iter().filter_map(|(_, state)| state.as_ref()))
                .map(|registration| registration.lifecycle.modified_at)
                .max();
            read_through = read_through.max(modified);
            let planned = self
                .membership
                .plan_reconciliation(&subscription, &matching, &departed, evaluated_through)
                .await;
            stream::iter(planned)
                .for_each_concurrent(CSR_NOTIFICATION_CONCURRENCY, |notification| async move {
                    self.send_planned(&notification).await;
                })
                .await;
        }
        Ok(read_through)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
