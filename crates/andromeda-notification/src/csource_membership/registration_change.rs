//! The notifications one committed registration change owes the change-driven
//! subscriptions of a tenant, each claimed in the record before it is sent.

use crate::{
    csource_membership::{CsourceMembership, PlannedNotification},
    csource_transition::{MembershipStep, RegistrationChange, Selection, StepOutcome, reason, step},
    error::NotificationError,
    notified_registration_store::{Release, UpdateReport},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, notification::CsrTriggerReason, registration::ContextSourceRegistration};
use andromeda_subscription::{
    compiled::CompiledCsrSubscription,
    csource_matching::{csource_subscription_follows_registration_changes, csource_subscription_selects_registration},
};
use std::{collections::HashSet, slice, sync::Arc};
use tracing::error;

impl CsourceMembership {
    /// The notifications one committed registration change owes the live,
    /// change-driven subscriptions among `subscriptions`, each recorded as
    /// notified.
    ///
    /// `before` is the registration as it stood before an update or deletion
    /// and `after` is the registration as it stands now, absent once deleted.
    /// A `noLongerMatching` notification carries `before`, the state the
    /// subscriber was told matched; without it, or when the subscription does
    /// not select it, the withdrawal is still recorded, but there is nothing
    /// matching to carry.
    pub(crate) async fn plan_registration_change<'registration>(
        &self,
        subscriptions: &[Arc<CompiledCsrSubscription>],
        change: RegistrationChange,
        revision: CsourceChangeRevision,
        registration_id: &Identifier,
        before: Option<&'registration ContextSourceRegistration>,
        after: Option<&'registration ContextSourceRegistration>,
    ) -> Vec<PlannedNotification<'registration>> {
        let notified = match self.subscriptions_notified_of(change, registration_id).await {
            Ok(notified) => notified,
            Err(failure) => {
                error!("Failed to read the subscriptions notified of CSR {}: {}", registration_id, failure);
                return Vec::new();
            }
        };
        let mut planned = Vec::new();
        for subscription in subscriptions
            .iter()
            .filter(|subscription| csource_subscription_follows_registration_changes(subscription))
        {
            let selection = match after {
                Some(current) if csource_subscription_selects_registration(subscription, current) => Selection::Selected,
                Some(_) | None => Selection::Unselected,
            };
            let outcome = match self.take_step(step(change, selection), &subscription.id, registration_id, &notified).await {
                Ok(outcome) => outcome,
                Err(failure) => {
                    error!("Failed to record CSR {} for subscription {}: {}", registration_id, subscription.id, failure);
                    continue;
                }
            };
            let previously = match before {
                Some(replaced) if csource_subscription_selects_registration(subscription, replaced) => Selection::Selected,
                Some(_) | None => Selection::Unselected,
            };
            let Some(reason) = reason(change, outcome, previously) else {
                continue;
            };
            let revision = match reason {
                CsrTriggerReason::Updated => match self.ledger.report_update(&subscription.id, registration_id, revision).await {
                    Ok(UpdateReport::Claimed) => Some(revision),
                    Ok(UpdateReport::Settled) => continue,
                    Err(failure) => {
                        error!(
                            "Failed to claim the update of CSR {} for subscription {}: {}",
                            registration_id, subscription.id, failure
                        );
                        continue;
                    }
                },
                CsrTriggerReason::NewlyMatching | CsrTriggerReason::NoLongerMatching => None,
            };
            // A notification carries only the `RegistrationInfo` elements the
            // subscription matches (clause 5.11.7), and clause 5.2.9 forbids an
            // empty `information`. Every change is evaluated in commit order, so
            // `before` filtered against the subscription is what the subscriber
            // was last told; when the subscription never selected `before`, the
            // record was written from a later state, and nothing matching is
            // left to carry.
            let carried = match (reason, previously) {
                (CsrTriggerReason::NewlyMatching | CsrTriggerReason::Updated, _) => after,
                (CsrTriggerReason::NoLongerMatching, Selection::Selected) => before,
                (CsrTriggerReason::NoLongerMatching, Selection::Unselected) => None,
            };
            if let Some(registration) = carried {
                planned.push(PlannedNotification {
                    subscription: Arc::clone(subscription),
                    registration,
                    reason,
                    change,
                    revision,
                });
            }
        }
        planned
    }

    /// The subscriptions a change may have to withdraw the registration from.
    ///
    /// A creation withdraws nothing, so it reads nothing.
    async fn subscriptions_notified_of(&self, change: RegistrationChange, registration: &Identifier) -> Result<HashSet<Identifier>, NotificationError> {
        match change {
            RegistrationChange::Created => Ok(HashSet::new()),
            RegistrationChange::Updated | RegistrationChange::Deleted => Ok(self.ledger.notified_subscriptions(registration).await?.into_iter().collect()),
        }
    }

    /// Apply `step` for `subscription` and report how the store answered it.
    async fn take_step(
        &self,
        step: MembershipStep,
        subscription: &Identifier,
        registration: &Identifier,
        notified: &HashSet<Identifier>,
    ) -> Result<StepOutcome, NotificationError> {
        match step {
            MembershipStep::Claim => {
                let recorded = self.ledger.claim(subscription, slice::from_ref(registration)).await?;
                Ok(if recorded.is_empty() {
                    StepOutcome::AlreadyRecorded
                } else {
                    StepOutcome::Recorded
                })
            }
            MembershipStep::Release if notified.contains(subscription) => Ok(match self.ledger.release(subscription, registration).await? {
                Release::Released => StepOutcome::Withdrawn,
                Release::Absent => StepOutcome::Unrecorded,
            }),
            MembershipStep::Release | MembershipStep::Keep => Ok(StepOutcome::Unrecorded),
        }
    }
}
