//! Delete Entity (clause 5.6.6) holds against the version it read, on both of
//! its paths.
//!
//! The tombstones of clauses 4.5.7 and 4.5.8 and the change clause 5.8.6
//! notifies describe the state the deletion read. These tests add an instance
//! to the Building between that read and the commit, as another broker
//! replica would, and prove the deletion is read again and ends the added
//! instance too.

use crate::compare_and_set_fixture::{Watchers, adds_attribute, building_id, contended_service, held_version, stored_building, tombstoned_terms};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::reads::attribute_change::AttributeChangeKind;
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex};

/// The terms `expected` names, as the fixture lists them.
fn terms(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|name| (*name).to_owned()).collect()
}

/// With no subscription to notify, the deletion reads only the outline, never
/// the state, and still ends the instance added after its first read.
#[tokio::test]
async fn a_deletion_from_the_outline_superseded_by_another_writer_ends_the_instance_it_added() {
    let contended = contended_service(Watchers::None).await;
    let id = building_id("a");
    contended.store.seed(&id, stored_building(&id, &["seed"]));
    let (_, read_version) = contended.store.stored(&id).expect("the Building is stored");
    let concurrent = Arc::new(Mutex::new(None));
    contended.store.interfere(adds_attribute(&id, "other", &concurrent));

    contended.service.delete(&ContextTarget::Live, &id).await.expect("the deletion lands");

    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2, "the superseded attempt and the one that landed");
    assert_eq!(held_version(&writes[0]), read_version);
    assert_eq!(Some(held_version(&writes[1])), *concurrent.lock().expect("the version lock is not poisoned"));
    assert_eq!(tombstoned_terms(&writes[0]), terms(&["seed"]));
    assert_eq!(tombstoned_terms(&writes[1]), terms(&["other", "seed"]));
    assert_eq!(contended.store.payload_reads(), 0, "the outline path never decodes the state");
    assert!(contended.store.stored(&id).is_none());
    assert_eq!(contended.published.lock().expect("recording mutex is not poisoned").len(), 1);
}

/// With a Building subscription to notify, the deletion reads the removed
/// state; the match and the tombstones of the attempt that landed are both
/// computed from the state that included the added instance.
#[tokio::test]
async fn a_deletion_from_the_removed_state_superseded_by_another_writer_notifies_the_instance_it_added() {
    let contended = contended_service(Watchers::Building).await;
    let id = building_id("a");
    contended.store.seed(&id, stored_building(&id, &["seed"]));
    contended.store.interfere(adds_attribute(&id, "other", &Arc::new(Mutex::new(None))));

    contended.service.delete(&ContextTarget::Live, &id).await.expect("the deletion lands");

    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2);
    assert_eq!(tombstoned_terms(&writes[1]), terms(&["other", "seed"]));
    let [matched] = writes[1].subscription_matches.as_slice() else {
        panic!("the Building subscription is owed the deletion: {:?}", writes[1].subscription_matches);
    };
    let mut deleted: Vec<String> = matched
        .change
        .instances
        .iter()
        .filter(|instance| instance.kind == AttributeChangeKind::Deleted)
        .map(|instance| instance.key.attr.as_str().to_owned())
        .collect();
    deleted.sort_unstable();
    assert_eq!(
        deleted,
        vec![
            "https://uri.etsi.org/ngsi-ld/default-context/other".to_owned(),
            "https://uri.etsi.org/ngsi-ld/default-context/seed".to_owned()
        ]
    );
    assert!(contended.store.stored(&id).is_none());
}
