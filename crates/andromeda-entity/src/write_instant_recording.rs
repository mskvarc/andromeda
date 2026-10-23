//! Clause 4.8 gives an instance one `createdAt` and one `modifiedAt`: every
//! write settles its system times at the instant it hands its commit, which
//! the commit records the write's history at, so current state and the
//! Temporal Evolution agree.

use crate::{
    admission_fixture::{admitted_attribute, admitted_context, admitted_entity, admitted_written},
    history_recording_assertions::{observed_default_instance, recorded_observation},
    merge_options::MergeOptions,
    notification_recording_harness::{
        CATEGORY_IRI,
        attribute_name,
        core_context,
        entity_uri,
        live_target,
        recording_service_counting_enqueued,
        single_recorded_write,
    },
    recording_ports::RecordedCommits,
    write_kind::{AppendAttributes, MergeEntity, PartialUpdateAttribute, ReplaceAttribute, ReplaceEntity, UpdateAttributes},
    write_target::WriteTarget,
};
use andromeda_context_commit::atomic_write::AtomicWrite;
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use andromeda_ngsi_ld_model::entity::EntityPatch;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::json;

/// The one commit `recorded` saw: its write and the instant it was accepted at.
fn single_commit(recorded: &RecordedCommits) -> (AtomicWrite, SystemInstant) {
    let accepted = recorded.accepted.lock().expect("recording mutex is not poisoned").clone(); // clone: the test owns the instants the recording keeps.
    assert_eq!(accepted.len(), 1, "one commit");
    (single_recorded_write(&recorded.writes), accepted[0])
}

/// The `modifiedAt` of the default instance of `category` the write records.
fn recorded_category_modified_at(write: &AtomicWrite) -> SystemInstant {
    let observation = recorded_observation(write).expect("an observation");
    let instance = observed_default_instance(&observation, CATEGORY_IRI);
    instance[properties::MODIFIED_AT.as_str()][0]["@value"]
        .as_str()
        .expect("a stamped modifiedAt")
        .parse()
        .expect("a system instant")
}

/// The commit of `recorded` was accepted at the instant its `category`
/// instance was settled at.
fn assert_category_settled_at_the_accepted_instant(recorded: &RecordedCommits) {
    let (write, accepted) = single_commit(recorded);
    assert_eq!(recorded_category_modified_at(&write), accepted);
}

#[tokio::test]
async fn partial_update_attr_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");
    assert_category_settled_at_the_accepted_instant(&recorded);
}

#[tokio::test]
async fn replace_attr_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<ReplaceAttribute>(&service, &attribute, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .replace_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");
    assert_category_settled_at_the_accepted_instant(&recorded);
}

#[tokio::test]
async fn update_attrs_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let admitted = admitted_written::<UpdateAttributes>(&service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
    service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the update lands");
    assert_category_settled_at_the_accepted_instant(&recorded);
}

#[tokio::test]
async fn append_attrs_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let admitted = admitted_written::<AppendAttributes>(&service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the append lands");
    assert_category_settled_at_the_accepted_instant(&recorded);
}

#[tokio::test]
async fn merge_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let fragment: EntityPatch<_> = serde_json::from_value(json!({"category": {"type": "Property", "value": "retail"}})).expect("a merge fragment");
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
    assert_category_settled_at_the_accepted_instant(&recorded);
}

#[tokio::test]
async fn replace_entity_settles_at_the_instant_its_commit_records_the_history_at() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let replacement: EntityPatch<_> =
        serde_json::from_value(json!({"type": "Building", "category": {"type": "Property", "value": "retail"}})).expect("a replacement");
    let admitted = admitted_entity::<ReplaceEntity>(&service, &replacement, &core_context()).await;
    service
        .replace_entity(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");
    assert_category_settled_at_the_accepted_instant(&recorded);
}
