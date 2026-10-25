//! Which context source notification a change owes each change-driven Context
//! Source Registration Subscription, decided against the registrations it was
//! already notified of.
//!
//! Clause 5.3.3 defines every context source trigger reason as a transition
//! against what the subscriber was already told: `newlyMatching` is used "in
//! the first notification and whenever a new Context Source Registration
//! matching the Subscription has been registered", while `updated` and
//! `noLongerMatching` concern a registration "that was part of a previous
//! notification". Deciding the reason from a re-read of the store instead
//! reports one registration twice when the initial notification of a new
//! subscription and the creation of a matching registration overlap, and
//! clause 5.11.7 owes one notification per change. So every notification is
//! claimed in the [`NotifiedRegistrationStore`] before it is sent: the store
//! records a registration as notified, or withdraws it, in the same step that
//! reports whether it already was, and a notification goes out only for the
//! step that changed it. Brokers sharing the tenant database claim against
//! the same record, so the step also decides which of them sends it. The
//! transition rules themselves live in [`crate::csource_transition`].

use crate::{
    csource_transition::{MembershipStep, RegistrationChange, Selection, StepOutcome, reason, step},
    error::NotificationError,
    notified_registration_store::{NotifiedRegistrationStore, Release},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration};
use andromeda_subscription::{
    change::UpsertKind,
    compiled::CompiledCsrSubscription,
    csource_matching::{csource_subscription_follows_registration_changes, csource_subscription_selects_registration},
    csource_notification_cadence::CsourceNotificationCadence,
};
use std::{collections::HashSet, slice, sync::Arc};
use tracing::error;

/// What a subscription upsert's notification carries, and what it recorded.
#[derive(Debug)]
pub(crate) struct UpsertPlan<'registration> {
    /// The registrations the notification carries.
    pub(crate) carried: Vec<&'registration ContextSourceRegistration>,
    /// The registrations the upsert newly recorded as notified.
    pub(crate) recorded: Vec<Identifier>,
}

/// One notification a registration change owes one subscription.
#[derive(Debug)]
pub(crate) struct PlannedNotification<'registration> {
    /// The subscription to notify.
    pub(crate) subscription: Arc<CompiledCsrSubscription>,
    /// The registration the notification carries.
    pub(crate) registration: &'registration ContextSourceRegistration,
    /// Why the subscription is notified.
    pub(crate) reason: CsrTriggerReason,
    /// The registration change the notification reports.
    pub(crate) change: RegistrationChange,
}

/// The registrations each change-driven subscription of one tenant was
/// notified of, and the notifications a change owes against them.
pub(crate) struct CsourceMembership {
    /// Where the notified registrations are recorded.
    ledger: Arc<dyn NotifiedRegistrationStore>,
}

impl CsourceMembership {
    /// Plan against the record `ledger` keeps.
    pub(crate) fn new(ledger: Arc<dyn NotifiedRegistrationStore>) -> CsourceMembership {
        CsourceMembership { ledger }
    }

    /// The registrations of `matching` the notification owed on a subscription
    /// upsert carries, and those it newly recorded as notified.
    ///
    /// A created subscription is told of every match not yet recorded for it
    /// (clause 5.11.2.4): a registration change evaluated before the initial
    /// notification has already reported the rest. An updated one is told of
    /// all current matches (clause 5.11.3.4). A `timeInterval` subscription is
    /// notified on its schedule and not by registration changes, so nothing
    /// is recorded for it.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the record cannot be
    /// written; nothing is then owed.
    pub(crate) async fn plan_upsert<'registration>(
        &self,
        subscription: &CompiledCsrSubscription,
        kind: UpsertKind,
        matching: &'registration [ContextSourceRegistration],
    ) -> Result<UpsertPlan<'registration>, NotificationError> {
        if !CsourceNotificationCadence::of(subscription).is_change_driven() {
            return Ok(UpsertPlan {
                carried: matching.iter().collect(),
                recorded: Vec::new(),
            });
        }
        let identifiers: Vec<Identifier> = matching.iter().map(|registration| registration.id.clone()).collect(); // clone: the store takes the identifiers as one slice
        let recorded = self.ledger.claim(&subscription.id, &identifiers).await?;
        let carried = match kind {
            UpsertKind::Created => {
                let newly: HashSet<&Identifier> = recorded.iter().collect();
                matching.iter().filter(|registration| newly.contains(&registration.id)).collect()
            }
            UpsertKind::Updated => matching.iter().collect(),
        };
        Ok(UpsertPlan { carried, recorded })
    }

    /// The registrations recorded for `subscription` that are not among
    /// `matching`, which an update of the subscription re-examines.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the record cannot be
    /// read.
    pub(crate) async fn recorded_beyond(
        &self,
        subscription: &Identifier,
        matching: &[ContextSourceRegistration],
    ) -> Result<Vec<Identifier>, NotificationError> {
        let current: HashSet<&Identifier> = matching.iter().map(|registration| &registration.id).collect();
        Ok(self
            .ledger
            .registrations_notified_to(subscription)
            .await?
            .into_iter()
            .filter(|registration| !current.contains(registration))
            .collect())
    }

    /// Withdraw each recorded registration the updated `subscription` no
    /// longer selects in the state `current` pairs it with, returning those
    /// withdrawn.
    ///
    /// One the subscription still selects was reported after its match set
    /// was read and stays recorded; withdrawing every record outside the match
    /// set wholesale would forget that report.
    pub(crate) async fn withdraw_unselected(
        &self,
        subscription: &CompiledCsrSubscription,
        current: Vec<(Identifier, Option<ContextSourceRegistration>)>,
    ) -> Vec<Identifier> {
        let mut withdrawn = Vec::new();
        for (identifier, state) in current {
            match state {
                Some(registration) if csource_subscription_selects_registration(subscription, &registration) => {}
                Some(_) | None => match self.ledger.release(&subscription.id, &identifier).await {
                    Ok(Release::Released) => withdrawn.push(identifier),
                    Ok(Release::Absent) => {}
                    Err(failure) => error!("Failed to withdraw CSR {} for subscription {}: {}", identifier, subscription.id, failure),
                },
            }
        }
        withdrawn
    }

    /// Record again the registrations an update withdrew, when its
    /// notification was not sent and the subscriber still believes they match.
    pub(crate) async fn restore(&self, subscription: &Identifier, registrations: &[Identifier]) {
        if let Err(failure) = self.ledger.claim(subscription, registrations).await {
            error!("Failed to restore the CSRs withdrawn from subscription {}: {}", subscription, failure);
        }
    }

    /// The registrations an upsert's notification recorded, confirmed against
    /// their state as re-read after the recording: `current` pairs each with
    /// what the store holds now. One deleted or changed out of matching since
    /// the match set was read is withdrawn and left out.
    ///
    /// The re-read closes the gap between reading the match set and recording
    /// it: a change committed inside that gap may already have been evaluated,
    /// found nothing recorded and withdrawn nothing, and a change committed
    /// after the recording is evaluated against it.
    pub(crate) async fn confirm(
        &self,
        subscription: &CompiledCsrSubscription,
        current: Vec<(Identifier, Option<ContextSourceRegistration>)>,
    ) -> Vec<ContextSourceRegistration> {
        let mut confirmed = Vec::with_capacity(current.len());
        for (identifier, state) in current {
            match state {
                Some(registration) if csource_subscription_selects_registration(subscription, &registration) => confirmed.push(registration),
                Some(_) | None => self.withdraw(&subscription.id, &identifier).await,
            }
        }
        confirmed
    }

    /// Withdraw the registrations a notification that was not sent had newly
    /// recorded, so a later change reports them as `newlyMatching`.
    pub(crate) async fn withdraw_unsent(&self, subscription: &Identifier, registrations: &[Identifier]) {
        for registration in registrations {
            self.withdraw(subscription, registration).await;
        }
    }

    /// Withdraw `registration` from those notified to `subscription`.
    async fn withdraw(&self, subscription: &Identifier, registration: &Identifier) {
        if let Err(failure) = self.ledger.release(subscription, registration).await {
            error!("Failed to withdraw CSR {} for subscription {}: {}", registration, subscription, failure);
        }
    }

    /// The notifications one committed registration change owes the live,
    /// change-driven subscriptions among `subscriptions`, each recorded as
    /// notified.
    ///
    /// `before` is the registration as it stood before an update or deletion
    /// and `after` is the registration as it stands now, absent once deleted.
    /// A `noLongerMatching` notification carries `before`, the state the
    /// subscriber was told matched; without it the withdrawal is still
    /// recorded, but there is nothing matching to carry.
    pub(crate) async fn plan_registration_change<'registration>(
        &self,
        subscriptions: &[Arc<CompiledCsrSubscription>],
        change: RegistrationChange,
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
            let Some(reason) = reason(change, outcome) else {
                continue;
            };
            let carried = match reason {
                CsrTriggerReason::NewlyMatching | CsrTriggerReason::Updated => after,
                CsrTriggerReason::NoLongerMatching => before,
            };
            if let Some(registration) = carried {
                planned.push(PlannedNotification {
                    subscription: Arc::clone(subscription),
                    registration,
                    reason,
                    change,
                });
            }
        }
        planned
    }

    /// Restore the record a planned notification that was not sent had
    /// changed, so the subscriber's view and the record agree again.
    ///
    /// A withdrawal owed for a deleted registration is not restored: the
    /// registration no longer exists, and a record of it would make a later
    /// registration created under the same identifier look already notified.
    pub(crate) async fn undo(&self, planned: &PlannedNotification<'_>) {
        let subscription = &planned.subscription.id;
        let registration = &planned.registration.id;
        let restored = match (planned.reason, planned.change) {
            (CsrTriggerReason::NewlyMatching, RegistrationChange::Created | RegistrationChange::Updated | RegistrationChange::Deleted) => {
                self.ledger.release(subscription, registration).await.map(drop)
            }
            (CsrTriggerReason::NoLongerMatching, RegistrationChange::Created | RegistrationChange::Updated) => {
                self.ledger.claim(subscription, slice::from_ref(registration)).await.map(drop)
            }
            (CsrTriggerReason::NoLongerMatching, RegistrationChange::Deleted)
            | (CsrTriggerReason::Updated, RegistrationChange::Created | RegistrationChange::Updated | RegistrationChange::Deleted) => Ok(()),
        };
        if let Err(failure) = restored {
            error!(
                "Failed to restore the record of unsent CSR {} for subscription {}: {}",
                registration, subscription, failure
            );
        }
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
