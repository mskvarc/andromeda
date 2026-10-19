//! Every whole-entity mutation records, for the Temporal Evolution, the
//! instances it left in the Entity rather than the Fragment that asked for them
//! (clauses 4.5.7 and 4.5.8), proven without a live broker.

use crate::{
    admission_fixture::{admitted_context, admitted_entity},
    history_recording_assertions::{assert_observes_stored_instances, observed_default_instance, recorded_observation},
    merge_options::MergeOptions,
    notification_recording_harness::{ACCURACY_IRI, CATEGORY_IRI, NAME_IRI, core_context, entity_uri, live_target, recording_service, single_recorded_write},
    write_kind::{MergeEntity, ReplaceEntity},
    write_target::WriteTarget,
};
use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::EntityPatch};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// Build an [`EntityPatch`] from a compact fixture payload; short-form terms
/// expand through the NGSI-LD default `@vocab` under the empty context.
fn patch<M: MemberReading>(payload: Value) -> EntityPatch<M> {
    serde_json::from_value(payload).expect("the fixture patch is a valid entity patch")
}

/// Clause 5.5.12: the merge keeps the `accuracy` sub-Property the Fragment
/// omits, so the recorded instance is the merged one carrying it.
#[tokio::test]
async fn merge_records_the_merged_instance_keeping_its_untouched_sub_property() {
    let (service, recorded) = recording_service().await;
    let fragment = patch(json!({"category": {"type": "Property", "value": "retail"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    let category = observed_default_instance(&observation, CATEGORY_IRI);
    assert_eq!(category[properties::VALUE.as_str()][0]["@value"], json!("retail"));
    assert_eq!(category[ACCURACY_IRI][0][properties::VALUE.as_str()][0]["@value"], json!("high"));
}

/// Clause 5.5.12: an NGSI-LD Null in a merge Fragment removes the member it
/// stands on. The recorded instance is the merged one without that member; the
/// sentinel is an instruction and never reaches history as a value.
#[tokio::test]
async fn merge_records_the_instance_without_the_sub_property_it_deleted_and_without_the_sentinel() {
    let (service, recorded) = recording_service().await;
    let fragment = patch(json!({"category": {"type": "Property", "value": "retail", "accuracy": "urn:ngsi-ld:null"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    assert!(observed_default_instance(&observation, CATEGORY_IRI).get(ACCURACY_IRI).is_none());
    assert!(
        !observation.to_string().contains("urn:ngsi-ld:null"),
        "no deletion sentinel is recorded: {observation}"
    );
}

/// Replace Entity records every attribute of the replacement as it is stored.
#[tokio::test]
async fn replace_entity_records_every_stored_attribute_of_the_replacement() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_entity::<ReplaceEntity>(
        &service,
        &patch(json!({
            "type": "Building",
            "category": {"type": "Property", "value": "retail"},
            "name": {"type": "Property", "value": "lobby"}
        })),
        &core_context(),
    )
    .await;
    service
        .replace_entity(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI, NAME_IRI]);
}

/// The fixture Building restated whole: its type and its `category` with the
/// `accuracy` sub-Property, as the store holds them.
fn restated_building() -> Value {
    json!({
        "type": "Building",
        "category": {"type": "Property", "value": "office", "accuracy": {"type": "Property", "value": "high"}}
    })
}

/// Clauses 4.3.1, 4.8 and 5.8.6: a Replace Entity restating the held entity
/// modifies nothing, so it stores no write and the entity keeps its
/// `modifiedAt`.
#[tokio::test]
async fn replace_entity_restating_the_held_entity_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_entity::<ReplaceEntity>(&service, &patch(restated_building()), &core_context()).await;
    service
        .replace_entity(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}

/// A Replace Entity restating `category` beside a new `name` records `name`
/// alone.
#[tokio::test]
async fn replace_entity_records_only_the_attribute_it_modified() {
    let (service, recorded) = recording_service().await;
    let mut replacement = restated_building();
    replacement["name"] = json!({"type": "Property", "value": "lobby"});
    let admitted = admitted_entity::<ReplaceEntity>(&service, &patch(replacement), &core_context()).await;
    service
        .replace_entity(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[NAME_IRI]);
}

/// Clause 5.5.12: a merge restating the held value modifies nothing, so it
/// stores no write.
#[tokio::test]
async fn merge_restating_the_held_value_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let fragment = patch(json!({"category": {"type": "Property", "value": "office"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}
