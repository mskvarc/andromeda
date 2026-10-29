//! The notifications a subscription is owed for the registration changes it did
//! not see, derived from the record of what it was notified of.

use crate::{
    csource_membership::{CsourceMembership, PlannedNotification},
    csource_transition::RegistrationChange,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration};
use andromeda_subscription::compiled::CompiledCsrSubscription;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, sync::Arc};
use tracing::error;

impl CsourceMembership {
    /// The notifications owed `subscription` for the registration changes it
    /// did not see, derived from the record instead of from the changes.
    ///
    /// `matching` is every registration the subscription selects now and
    /// `departed` pairs each registration recorded for it outside that set with
    /// its state as the store holds it, absent once deleted. A match not yet
    /// recorded is recorded and reported as `newlyMatching` (clause 5.3.3). A
    /// departed registration no longer selected is withdrawn and reported as
    /// `noLongerMatching` with the state it has now, since the state the
    /// subscriber was told matched is not known; one that was deleted is
    /// withdrawn without a notification, as a deletion that carries no prior
    /// state is. The record does not keep what the subscriber was last told, so
    /// a recorded match is reported as `updated` when it was modified after
    /// `evaluated_through`, the modification time of the last change evaluated
    /// before the gap; without one, nothing is known to have changed.
    pub(crate) async fn plan_reconciliation<'registration>(
        &self,
        subscription: &Arc<CompiledCsrSubscription>,
        matching: &'registration [ContextSourceRegistration],
        departed: &'registration [(Identifier, Option<ContextSourceRegistration>)],
        evaluated_through: Option<DateTime<Utc>>,
    ) -> Vec<PlannedNotification<'registration>> {
        let identifiers: Vec<Identifier> = matching.iter().map(|registration| registration.id.clone()).collect(); // clone: the store takes the identifiers as one slice
        let recorded = match self.ledger.claim(&subscription.id, &identifiers).await {
            Ok(recorded) => recorded.into_iter().collect::<HashSet<Identifier>>(),
            Err(failure) => {
                error!("Failed to record the CSRs matching subscription {}: {}", subscription.id, failure);
                HashSet::new()
            }
        };
        let withdrawn: HashSet<Identifier> = self.withdraw_unselected(subscription, departed).await.into_iter().collect();
        let newly = matching
            .iter()
            .filter(|registration| recorded.contains(&registration.id))
            .map(|registration| (registration, CsrTriggerReason::NewlyMatching));
        let changed = matching
            .iter()
            .filter(|registration| !recorded.contains(&registration.id))
            .filter(|registration| evaluated_through.is_some_and(|through| registration.lifecycle.modified_at > through))
            .map(|registration| (registration, CsrTriggerReason::Updated));
        let gone = departed
            .iter()
            .filter(|(identifier, _)| withdrawn.contains(identifier))
            .filter_map(|(_, state)| state.as_ref())
            .map(|registration| (registration, CsrTriggerReason::NoLongerMatching));
        newly
            .chain(changed)
            .chain(gone)
            .map(|(registration, reason)| PlannedNotification {
                subscription: Arc::clone(subscription),
                registration,
                reason,
                change: RegistrationChange::Updated,
                revision: None,
            })
            .collect()
    }
}
