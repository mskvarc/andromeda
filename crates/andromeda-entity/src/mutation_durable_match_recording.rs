//! Every whole-entity mutation records the durable subscription matches its own
//! live notification would produce (clause 5.8), proven without a live broker.
//!
//! The single merge (clause 5.6.17) and replace (clause 5.6.18) paths each
//! record one match carrying the precise instance transitions the mutation
//! made -- never a coarse `entityUpdated`, and never one match per changed
//! attribute (clause 5.8.6). These tests commit each operation against the
//! shared recording harness and read the `subscription_matches` off the
//! [`AtomicWrite`](andromeda_context_commit::atomic_write::AtomicWrite) it
//! produced, so a regression to a whole-entity trigger would fail loudly rather
//! than only under an HTTP-status integration test.

use crate::{
    admission_fixture::admitted_entity,
    merge_options::MergeOptions,
    notification_recording_harness::{
        CATEGORY_IRI,
        NAME_IRI,
        assert_single_match,
        core_context,
        entity_uri,
        live_target,
        recording_service,
        single_recorded_write,
    },
    write_kind::{MergeEntity, ReplaceEntity},
};
use andromeda_ngsi_ld_model::{entity::EntityPatch, expanded::attribute_change::AttributeChangeKind};
use serde_json::json;

/// Build an [`EntityPatch`] from a compact fixture payload; short-form terms
/// expand through the NGSI-LD default `@vocab` under the empty context.
fn patch(payload: serde_json::Value) -> EntityPatch {
    serde_json::from_value(payload).expect("the fixture patch is a valid entity patch")
}

#[tokio::test]
async fn merge_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let fragment = patch(json!({"category": {"type": "Property", "value": "retail"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(&live_target(), &entity_uri(), &fragment, admitted, &core_context(), MergeOptions::default())
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Updated, CATEGORY_IRI)]);
}

#[tokio::test]
async fn merge_records_an_attribute_created_match() {
    let (service, recorded) = recording_service().await;
    let fragment = patch(json!({"name": {"type": "Property", "value": "lobby"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(&live_target(), &entity_uri(), &fragment, admitted, &core_context(), MergeOptions::default())
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Created, NAME_IRI)]);
}

#[tokio::test]
async fn replace_entity_records_the_replacement_diff() {
    let (service, recorded) = recording_service().await;
    // The replacement omits `category`, so the diff against the stored Building
    // deletes it and creates `name` (clause 5.6.18): one match carrying both.
    let admitted = admitted_entity::<ReplaceEntity>(
        &service,
        &patch(json!({"type": "Building", "name": {"type": "Property", "value": "lobby"}})),
        &core_context(),
    )
    .await;
    service
        .replace_entity(&live_target(), &entity_uri(), admitted)
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(
        &write,
        &[(AttributeChangeKind::Deleted, CATEGORY_IRI), (AttributeChangeKind::Created, NAME_IRI)],
    );
}
