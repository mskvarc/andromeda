use crate::csource_transition::{ChangeStates, MembershipStep, RegistrationChange, Selection, StepOutcome, reason, step};
use andromeda_ngsi_ld_model::{
    notification::{CsrChange, CsrTriggerReason},
    registration::ContextSourceRegistration,
};
use pretty_assertions::assert_eq;
use serde_json::json;

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
        (
            RegistrationChange::Created,
            StepOutcome::Recorded,
            Selection::Unselected,
            Some(CsrTriggerReason::NewlyMatching),
        ),
        (RegistrationChange::Created, StepOutcome::AlreadyRecorded, Selection::Unselected, None),
        (RegistrationChange::Created, StepOutcome::Unrecorded, Selection::Unselected, None),
        (
            RegistrationChange::Updated,
            StepOutcome::Recorded,
            Selection::Unselected,
            Some(CsrTriggerReason::NewlyMatching),
        ),
        (
            RegistrationChange::Updated,
            StepOutcome::Recorded,
            Selection::Selected,
            Some(CsrTriggerReason::NewlyMatching),
        ),
        (
            RegistrationChange::Updated,
            StepOutcome::AlreadyRecorded,
            Selection::Selected,
            Some(CsrTriggerReason::Updated),
        ),
        (
            RegistrationChange::Updated,
            StepOutcome::Withdrawn,
            Selection::Selected,
            Some(CsrTriggerReason::NoLongerMatching),
        ),
        (RegistrationChange::Updated, StepOutcome::Unrecorded, Selection::Selected, None),
        (
            RegistrationChange::Deleted,
            StepOutcome::Withdrawn,
            Selection::Selected,
            Some(CsrTriggerReason::NoLongerMatching),
        ),
        (RegistrationChange::Deleted, StepOutcome::Unrecorded, Selection::Selected, None),
    ];
    for (change, outcome, previously, expected) in cases {
        assert_eq!(reason(change, outcome, previously), expected, "{change:?} / {outcome:?} / {previously:?}");
    }
}

/// Clause 5.3.3: an update that brought a registration into the selection owes
/// `newlyMatching` once; a second evaluation of it finds the registration
/// recorded and owes nothing, never `updated`.
#[test]
fn an_update_into_the_selection_is_not_reported_as_updated_when_already_recorded() {
    assert_eq!(reason(RegistrationChange::Updated, StepOutcome::AlreadyRecorded, Selection::Unselected), None);
}

/// A registration with `id` covering `entity_type`.
fn registration(id: &str, entity_type: &str) -> Box<ContextSourceRegistration> {
    Box::new(
        serde_json::from_value(json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": entity_type}]}],
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration"),
    )
}

/// The type the first entity selector of `registration` names.
fn covered_type(registration: Option<&ContextSourceRegistration>) -> Option<String> {
    registration.and_then(|registration| {
        registration
            .coverage
            .information
            .first()
            .and_then(|info| info.entities.as_ref())
            .and_then(|entities| entities.first())
            .map(|entity| entity.type_.as_type_expression().to_string())
    })
}

/// A creation is evaluated against the registration it wrote, with nothing
/// before it.
#[test]
fn a_creation_carries_the_registration_it_wrote() {
    let change = CsrChange::Created {
        written: registration("urn:ngsi-ld:ContextSourceRegistration:1", "https://example.org/VehicleO"),
    };

    let states = ChangeStates::of(&change);

    assert_eq!(states.change, RegistrationChange::Created);
    assert_eq!(covered_type(states.before), None);
    assert_eq!(covered_type(states.after), Some("https://example.org/VehicleO".to_owned()));
}

/// An update is evaluated between the registration it replaced and the one it
/// wrote.
#[test]
fn an_update_carries_the_registration_it_replaced_and_the_one_it_wrote() {
    let change = CsrChange::Updated {
        before: registration("urn:ngsi-ld:ContextSourceRegistration:1", "https://example.org/VehicleO"),
        written: registration("urn:ngsi-ld:ContextSourceRegistration:1", "https://example.org/VehicleR"),
    };

    let states = ChangeStates::of(&change);

    assert_eq!(states.change, RegistrationChange::Updated);
    assert_eq!(covered_type(states.before), Some("https://example.org/VehicleO".to_owned()));
    assert_eq!(covered_type(states.after), Some("https://example.org/VehicleR".to_owned()));
}

/// A deletion leaves nothing after it, and carries the registration it removed
/// when that was read first.
#[test]
fn a_deletion_carries_only_the_registration_it_removed() {
    let read_first = CsrChange::Deleted {
        before: Some(registration("urn:ngsi-ld:ContextSourceRegistration:1", "https://example.org/VehicleR")),
    };
    let unread = CsrChange::Deleted { before: None };

    let read_first = ChangeStates::of(&read_first);
    let unread = ChangeStates::of(&unread);

    assert_eq!(read_first.change, RegistrationChange::Deleted);
    assert_eq!(covered_type(read_first.before), Some("https://example.org/VehicleR".to_owned()));
    assert_eq!(covered_type(read_first.after), None);
    assert_eq!(unread.change, RegistrationChange::Deleted);
    assert_eq!(covered_type(unread.before), None);
}
