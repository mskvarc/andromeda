//! Every attribute-level write records the durable subscription matches its own
//! live notification would produce (clause 5.8), proven without a live broker.
//!
//! Each of the five clause 5.6.2 to 5.6.5 and 5.6.19 operations computes its
//! notification inside the same closure that computes the write. These tests
//! commit each operation against the shared recording harness and read the
//! `subscription_matches` off the [`AtomicWrite`](andromeda_context_commit::atomic_write::AtomicWrite)
//! it produced, so a matching subscription is owed exactly the trigger,
//! `changedAttributes` and snapshot the operation's live emit would carry.

use crate::notification_recording_harness::{
    CATEGORY_IRI,
    NAME_IRI,
    assert_single_match,
    attribute_name,
    core_context,
    entity_uri,
    recording_service,
    single_recorded_write,
};
use andromeda_ngsi_ld_model::{
    expanded::mutation::selection::{DatasetSelection, OverwritePolicy},
    notification::NotificationTrigger,
};
use serde_json::json;

#[tokio::test]
async fn append_attrs_records_an_attribute_created_match() {
    let (service, recorded) = recording_service().await;
    service
        .append_attrs(
            &entity_uri(),
            json!({"name": {"type": "Property", "value": "lobby"}}),
            OverwritePolicy::Overwrite,
            &core_context(),
        )
        .await
        .expect("the append lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeCreated, NAME_IRI);
}

#[tokio::test]
async fn update_attrs_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .update_attrs(&entity_uri(), json!({"category": {"type": "Property", "value": "retail"}}), &core_context())
        .await
        .expect("the update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn partial_update_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .partial_update_attr(
            &entity_uri(),
            &attribute_name("category"),
            None,
            json!({"type": "Property", "value": "retail"}),
            &core_context(),
        )
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn delete_attr_records_an_attribute_deleted_match() {
    let (service, recorded) = recording_service().await;
    service
        .delete_attr(&entity_uri(), &attribute_name("category"), DatasetSelection::Default, &core_context())
        .await
        .expect("the delete lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeDeleted, CATEGORY_IRI);
}

#[tokio::test]
async fn replace_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .replace_attr(
            &entity_uri(),
            &attribute_name("category"),
            None,
            json!({"type": "Property", "value": "retail"}),
            &core_context(),
        )
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}
