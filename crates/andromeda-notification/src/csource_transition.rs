//! The transitions clause 5.3.3 defines for context source notifications: the
//! step a registration change takes against what one subscription was already
//! notified of, and the trigger reason the outcome of that step owes.
//!
//! Pure rules, decided from the kind of change, whether the subscription now
//! selects the registration, and how the notified-registration record answered;
//! [`crate::csource_membership`] applies them against the record.

use andromeda_ngsi_ld_model::notification::CsrTriggerReason;

/// The mutation a committed registration change applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegistrationChange {
    /// The registration was created.
    Created,
    /// The registration was updated.
    Updated,
    /// The registration was deleted.
    Deleted,
}

impl RegistrationChange {
    /// The mutation a registration change event names by its trigger reason.
    ///
    /// The registration store publishes a creation as `newlyMatching`, an
    /// update as `updated` and a deletion as `noLongerMatching`; which reason
    /// a given subscription is notified with is decided per subscription.
    pub(crate) const fn from_event_reason(reason: CsrTriggerReason) -> RegistrationChange {
        match reason {
            CsrTriggerReason::NewlyMatching => RegistrationChange::Created,
            CsrTriggerReason::Updated => RegistrationChange::Updated,
            CsrTriggerReason::NoLongerMatching => RegistrationChange::Deleted,
        }
    }
}

/// Whether a subscription selects a registration in its current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    /// The registration matches the subscription.
    Selected,
    /// The registration does not match the subscription, or no longer exists.
    Unselected,
}

/// What a registration change does to the registrations one subscription was
/// notified of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MembershipStep {
    /// Record the registration as notified.
    Claim,
    /// Withdraw the registration from those notified.
    Release,
    /// Leave the record as it is.
    Keep,
}

/// The step `change` takes for a subscription that now `selection`s the
/// registration.
///
/// A creation that does not match leaves the record alone even when the
/// registration was already notified: that can only be an initial
/// notification that read a later state, and the update producing that state
/// either is evaluated against the record, or committed before the initial
/// notification re-read the registration, which then withdrew it.
pub(crate) const fn step(change: RegistrationChange, selection: Selection) -> MembershipStep {
    match (change, selection) {
        (RegistrationChange::Created | RegistrationChange::Updated, Selection::Selected) => MembershipStep::Claim,
        (RegistrationChange::Updated, Selection::Unselected) | (RegistrationChange::Deleted, Selection::Selected | Selection::Unselected) => {
            MembershipStep::Release
        }
        (RegistrationChange::Created, Selection::Unselected) => MembershipStep::Keep,
    }
}

/// How the store answered a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepOutcome {
    /// The claim recorded the registration.
    Recorded,
    /// The claim found the registration already recorded.
    AlreadyRecorded,
    /// The release withdrew the registration.
    Withdrawn,
    /// The release found the registration was never recorded, or the record
    /// was kept.
    Unrecorded,
}

/// The trigger reason a change notifies a subscription with, given how the
/// store answered its step, or `None` when the change alters nothing the
/// subscriber was told.
///
/// A registration already recorded is not reported again by its creation: the
/// notification that recorded it is the one clause 5.11.7 owes for it.
pub(crate) const fn reason(change: RegistrationChange, outcome: StepOutcome) -> Option<CsrTriggerReason> {
    match (change, outcome) {
        (RegistrationChange::Created | RegistrationChange::Updated | RegistrationChange::Deleted, StepOutcome::Recorded) => {
            Some(CsrTriggerReason::NewlyMatching)
        }
        (RegistrationChange::Updated, StepOutcome::AlreadyRecorded) => Some(CsrTriggerReason::Updated),
        (RegistrationChange::Created | RegistrationChange::Updated | RegistrationChange::Deleted, StepOutcome::Withdrawn) => {
            Some(CsrTriggerReason::NoLongerMatching)
        }
        (RegistrationChange::Created | RegistrationChange::Deleted, StepOutcome::AlreadyRecorded)
        | (RegistrationChange::Created | RegistrationChange::Updated | RegistrationChange::Deleted, StepOutcome::Unrecorded) => None,
    }
}

#[cfg(test)]
mod tests;
