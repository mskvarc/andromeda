use crate::csource_transition::{MembershipStep, RegistrationChange, Selection, StepOutcome, reason, step};
use andromeda_ngsi_ld_model::notification::CsrTriggerReason;
use pretty_assertions::assert_eq;

/// Every mutation and selection takes the step that records, withdraws or
/// keeps the registration.
#[test]
fn every_change_and_selection_takes_one_step() {
    let cases = [
        (RegistrationChange::Created, Selection::Selected, MembershipStep::Claim),
        (RegistrationChange::Created, Selection::Unselected, MembershipStep::Keep),
        (RegistrationChange::Updated, Selection::Selected, MembershipStep::Claim),
        (RegistrationChange::Updated, Selection::Unselected, MembershipStep::Release),
        (RegistrationChange::Deleted, Selection::Selected, MembershipStep::Release),
        (RegistrationChange::Deleted, Selection::Unselected, MembershipStep::Release),
    ];
    for (change, selection, expected) in cases {
        assert_eq!(step(change, selection), expected, "{change:?} / {selection:?}");
    }
}

/// Clause 5.3.3: every change and store answer decides one reason, or none.
#[test]
fn every_store_answer_decides_one_reason() {
    let cases = [
        (RegistrationChange::Created, StepOutcome::Recorded, Some(CsrTriggerReason::NewlyMatching)),
        (RegistrationChange::Created, StepOutcome::AlreadyRecorded, None),
        (RegistrationChange::Created, StepOutcome::Unrecorded, None),
        (RegistrationChange::Updated, StepOutcome::Recorded, Some(CsrTriggerReason::NewlyMatching)),
        (RegistrationChange::Updated, StepOutcome::AlreadyRecorded, Some(CsrTriggerReason::Updated)),
        (RegistrationChange::Updated, StepOutcome::Withdrawn, Some(CsrTriggerReason::NoLongerMatching)),
        (RegistrationChange::Updated, StepOutcome::Unrecorded, None),
        (RegistrationChange::Deleted, StepOutcome::Withdrawn, Some(CsrTriggerReason::NoLongerMatching)),
        (RegistrationChange::Deleted, StepOutcome::Unrecorded, None),
    ];
    for (change, outcome, expected) in cases {
        assert_eq!(reason(change, outcome), expected, "{change:?} / {outcome:?}");
    }
}

/// The store publishes creation, update and deletion under the reason a
/// subscriber that never saw the registration would be told.
#[test]
fn an_event_reason_names_the_mutation_it_was_published_for() {
    assert_eq!(
        RegistrationChange::from_event_reason(CsrTriggerReason::NewlyMatching),
        RegistrationChange::Created
    );
    assert_eq!(RegistrationChange::from_event_reason(CsrTriggerReason::Updated), RegistrationChange::Updated);
    assert_eq!(
        RegistrationChange::from_event_reason(CsrTriggerReason::NoLongerMatching),
        RegistrationChange::Deleted
    );
}
