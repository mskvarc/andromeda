//! Every whole-entity mutation records the durable subscription matches its own
//! live notification would produce (clause 5.8), proven without a live broker.
//!
//! The single merge (clause 5.6.17) and replace (clause 5.6.18) paths each fire
//! per-attribute triggers — one match per attribute the mutation changed — never
//! a coarse `entityUpdated`. These tests commit each operation against the
//! shared recording harness and read the `subscription_matches` off the
//! [`AtomicWrite`](andromeda_context_commit::atomic_write::AtomicWrite) it
//! produced, so a regression to a whole-entity trigger would fail loudly rather
//! than only under an HTTP-status integration test.

use crate::{
    merge_options::MergeOptions,
    notification_recording_harness::{
        CATEGORY_IRI,
        NAME_IRI,
        assert_matches,
        assert_single_match,
        core_context,
        entity_uri,
        recording_service,
        single_recorded_write,
    },
};
use andromeda_ngsi_ld_model::{entity::EntityPatch, notification::NotificationTrigger};
use serde_json::json;

/// Build an [`EntityPatch`] from a compact fixture payload; short-form terms
/// expand through the NGSI-LD default `@vocab` under the empty context.
fn patch(payload: serde_json::Value) -> EntityPatch {
    serde_json::from_value(payload).expect("the fixture patch is a valid entity patch")
}

#[tokio::test]
async fn merge_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .merge(
            &entity_uri(),
            &patch(json!({"category": {"type": "Property", "value": "retail"}})),
            &core_context(),
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn merge_records_an_attribute_created_match() {
    let (service, recorded) = recording_service().await;
    service
        .merge(
            &entity_uri(),
            &patch(json!({"name": {"type": "Property", "value": "lobby"}})),
            &core_context(),
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeCreated, NAME_IRI);
}

#[tokio::test]
async fn replace_entity_records_the_replacement_diff() {
    let (service, recorded) = recording_service().await;
    // The replacement omits `category`, so the diff against the stored Building
    // deletes it and creates `name` (clause 5.6.18): two per-attribute matches.
    service
        .replace_entity(
            &entity_uri(),
            &patch(json!({"type": "Building", "name": {"type": "Property", "value": "lobby"}})),
            &core_context(),
        )
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_matches(
        &write,
        &[
            (NotificationTrigger::AttributeDeleted, CATEGORY_IRI),
            (NotificationTrigger::AttributeCreated, NAME_IRI),
        ],
    );
}
