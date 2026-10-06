//! Every attribute-level write records the durable subscription matches its own
//! live notification would produce (clause 5.8), proven without a live broker.
//!
//! Each of the five clause 5.6.2 to 5.6.5 and 5.6.19 operations computes its
//! notification inside the same closure that computes the write. These tests
//! commit each operation against the shared recording harness and read the
//! `subscription_matches` off the [`AtomicWrite`](andromeda_context_commit::atomic_write::AtomicWrite)
//! it produced, so a matching subscription is owed exactly the trigger,
//! `changedAttributes` and snapshot the operation's live emit would carry.

use crate::{
    admission_fixture::{admitted_attribute, admitted_written},
    notification_recording_harness::{
        CATEGORY_IRI,
        NAME_IRI,
        assert_single_match,
        attribute_name,
        core_context,
        entity_uri,
        live_target,
        recording_service,
        single_recorded_write,
    },
    write_kind::{AppendAttributes, PartialUpdateAttribute, ReplaceAttribute, UpdateAttributes},
    write_target::WriteTarget,
};
use andromeda_ngsi_ld_entity::{
    operation::selection::{DatasetSelection, OverwritePolicy},
    reads::attribute_change::AttributeChangeKind,
};
use serde_json::json;

#[tokio::test]
async fn append_attrs_records_an_attribute_created_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<AppendAttributes>(&service, json!({"name": {"type": "Property", "value": "lobby"}}), &core_context()).await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the append lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Created, NAME_IRI)]);
}

#[tokio::test]
async fn update_attrs_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<UpdateAttributes>(&service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
    service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Updated, CATEGORY_IRI)]);
}

#[tokio::test]
async fn partial_update_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let category = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &category, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Updated, CATEGORY_IRI)]);
}

#[tokio::test]
async fn delete_attr_records_an_attribute_deleted_match() {
    let (service, recorded) = recording_service().await;
    service
        .delete_attr(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &attribute_name("category"),
            DatasetSelection::Default,
            &core_context(),
        )
        .await
        .expect("the delete lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Deleted, CATEGORY_IRI)]);
}

#[tokio::test]
async fn replace_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let category = attribute_name("category");
    let admitted = admitted_attribute::<ReplaceAttribute>(&service, &category, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .replace_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Updated, CATEGORY_IRI)]);
}
