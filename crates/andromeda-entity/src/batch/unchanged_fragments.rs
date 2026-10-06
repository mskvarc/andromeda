//! A batch entity whose fragment changes nothing stores no write, and is still
//! reported as a success.
//!
//! Clauses 5.6.8.4 (update mode), 5.6.9.4 and 5.6.20.4 run Update Attributes,
//! Append Attributes and Merge Entity per entity, and clause 5.4 makes a
//! Fragment an IETF RFC 7396 JSON Merge Patch, which returns its target
//! unchanged when it names nothing to change. The entity therefore keeps its
//! `modifiedAt` (clause 4.8), notifies nobody and records no Temporal
//! Evolution instance, while clauses 5.6.9.5 and 5.6.20.5 still list it among
//! the entities successfully processed. A fragment that restates a value the
//! entity holds names an instance, so it records that instance's observation
//! exactly as the single-entity operation does and is therefore stored.
//! Replace mode (clause 5.6.8.4 runs Replace Entity, clause 5.6.18) is a whole
//! replacement and always writes.

use crate::{
    admission_fixture::admitted_batch,
    batch::{BatchResult, UpsertPolicy},
    building_subscription_fixture::BUILDING_TYPE_IRI,
    compare_and_set_fixture::{ContendedService, Watchers, attribute_terms, building_id, contended_service, request_building, term},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_kind::{BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, operation::selection::OverwritePolicy};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The branch every test writes.
const LIVE: ContextTarget = ContextTarget::Live;

/// A fragment naming no member beside the entity's identity.
fn empty_fragment() -> Value {
    json!({})
}

/// A fragment restating the value the stored `seed` Property already holds.
fn identical_fragment() -> Value {
    json!({"seed": {"type": "Property", "value": 1}})
}

/// A fragment naming a Property the stored entity does not hold.
fn new_attribute_fragment() -> Value {
    json!({"fresh": {"type": "Property", "value": 7}})
}

/// The Building `id` as the store holds it after a write: its `seed` Property
/// carries the clause 4.8 `createdAt` and `modifiedAt` every stored instance has.
fn stamped_building(id: &Identifier) -> EntityState {
    let stamp = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}]);
    let stored = json!([{
        "@id": id.as_str(),
        "@type": [BUILDING_TYPE_IRI],
        term("seed"): [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/createdAt": stamp,
            "https://uri.etsi.org/ngsi-ld/modifiedAt": stamp
        }]
    }]);
    EntityState::from_slice(&serde_json::to_vec(&stored).expect("the fixture is JSON")).expect("the fixture is a stored entity")
}

/// A service whose store holds the Building `a` with a `seed` Property, and
/// the version it holds it at.
async fn seeded() -> (ContendedService, Identifier, DateTime<Utc>) {
    let contended = contended_service(Watchers::None).await;
    let id = building_id("a");
    contended.store.seed(&id, stamped_building(&id));
    let (_, version) = contended.store.stored(&id).expect("the Building is stored");
    (contended, id, version)
}

/// Update the Building `id` with `fragment` in one batch, inline.
async fn batch_update(contended: &ContendedService, id: &Identifier, fragment: &Value, overwrite: OverwritePolicy) -> BatchResult {
    let admitted = admitted_batch(&contended.service, BatchUpdate, vec![request_building(id, fragment)], &ContextSpec::core_only()).await;
    contended
        .service
        .batch_update(SideEffectPlacement::Inline(&LIVE), admitted, overwrite)
        .await
        .expect("the batch completes")
}

/// Merge `fragment` into the Building `id` in one batch, inline.
async fn batch_merge(contended: &ContendedService, id: &Identifier, fragment: &Value) -> BatchResult {
    let admitted = admitted_batch(&contended.service, BatchMerge, vec![request_building(id, fragment)], &ContextSpec::core_only()).await;
    contended
        .service
        .batch_merge(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes")
}

/// Upsert `fragment` into the Building `id` in one batch under `policy`, inline.
async fn batch_upsert(contended: &ContendedService, id: &Identifier, fragment: &Value, policy: UpsertPolicy) -> (BatchResult, Vec<Identifier>) {
    let admitted = admitted_batch(
        &contended.service,
        BatchUpsert(policy),
        vec![request_building(id, fragment)],
        &ContextSpec::core_only(),
    )
    .await;
    contended
        .service
        .batch_upsert(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes")
}

/// Assert the batch stored nothing for `id`: no write reached the store, the
/// entity kept its version and no change was published.
fn assert_nothing_stored(contended: &ContendedService, id: &Identifier, version: DateTime<Utc>) {
    let submitted = contended.store.submitted_for(id);
    assert!(submitted.is_empty(), "{submitted:?}");
    assert_eq!(
        contended.store.stored(id).expect("the Building is stored").1,
        version,
        "modifiedAt is untouched"
    );
    assert!(
        contended.published.lock().expect("recording mutex is not poisoned").is_empty(),
        "no change is published"
    );
}

/// Assert the batch stored exactly one write for `id`, recording an observation.
fn assert_observation_recorded(contended: &ContendedService, id: &Identifier) {
    let submitted = contended.store.submitted_for(id);
    assert_eq!(submitted.len(), 1);
    let write = submitted.first().expect("one write was submitted");
    assert!(write.history.iter().any(HistoryRecord::is_observations), "{:?}", write.history);
}

/// Assert the batch answered `id` as the one success, with no error.
fn assert_sole_success(result: &BatchResult, id: &Identifier) {
    assert_eq!(result.success, vec![id.clone()]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

/// Assert the batch stored exactly one write for `id`, which added `fresh`.
fn assert_fresh_attribute_stored(contended: &ContendedService, id: &Identifier, version: DateTime<Utc>) {
    assert_eq!(contended.store.submitted_for(id).len(), 1);
    let (state, stored_version) = contended.store.stored(id).expect("the Building is stored");
    assert_eq!(attribute_terms(&state), vec!["fresh".to_owned(), "seed".to_owned()]);
    assert!(stored_version > version, "modifiedAt moves with a real change");
}

/// Clause 5.6.9.4: an overwriting update whose fragment names no member
/// changes nothing.
#[tokio::test]
async fn a_batch_update_with_an_empty_fragment_stores_nothing_and_reports_success() {
    let (contended, id, version) = seeded().await;

    let result = batch_update(&contended, &id, &empty_fragment(), OverwritePolicy::Overwrite).await;

    assert_sole_success(&result, &id);
    assert_nothing_stored(&contended, &id, version);
}

/// Clause 5.6.9.4: an overwriting update restating the stored value names an
/// instance, whose observation the Temporal Evolution records as it does for
/// the single Update Attributes.
#[tokio::test]
async fn a_batch_update_restating_the_stored_value_still_records_the_observation() {
    let (contended, id, _) = seeded().await;

    let result = batch_update(&contended, &id, &identical_fragment(), OverwritePolicy::Overwrite).await;

    assert_sole_success(&result, &id);
    assert_observation_recorded(&contended, &id);
}

/// Clause 5.6.9.4 with `noOverwrite`: an empty fragment appends nothing and is
/// not a conflict.
#[tokio::test]
async fn a_no_overwrite_batch_update_with_an_empty_fragment_stores_nothing_and_reports_success() {
    let (contended, id, version) = seeded().await;

    let result = batch_update(&contended, &id, &empty_fragment(), OverwritePolicy::Preserve).await;

    assert_sole_success(&result, &id);
    assert_nothing_stored(&contended, &id, version);
}

/// Clause 5.6.9.5: a `noOverwrite` update naming only attributes the entity
/// holds appends nothing, so it stores nothing and reports the conflict.
#[tokio::test]
async fn a_no_overwrite_batch_update_of_held_attributes_stores_nothing_and_reports_the_conflict() {
    let (contended, id, version) = seeded().await;

    let result = batch_update(&contended, &id, &identical_fragment(), OverwritePolicy::Preserve).await;

    assert_nothing_stored(&contended, &id, version);
    let conflicts: Vec<&EntityError> = result.errors.iter().filter(|(failed, _)| *failed == id).map(|(_, error)| error).collect();
    assert!(
        matches!(conflicts.as_slice(), [EntityError::Validation(EntityValidationReason::NoOverwriteConflict)]),
        "{conflicts:?}"
    );
}

/// A real change still writes, with and without `noOverwrite`.
#[tokio::test]
async fn a_batch_update_that_adds_an_attribute_still_writes() {
    for overwrite in [OverwritePolicy::Overwrite, OverwritePolicy::Preserve] {
        let (contended, id, version) = seeded().await;

        let result = batch_update(&contended, &id, &new_attribute_fragment(), overwrite).await;

        assert_sole_success(&result, &id);
        assert_fresh_attribute_stored(&contended, &id, version);
    }
}

/// Clause 5.6.20.4: a merge whose fragment names no member changes nothing.
#[tokio::test]
async fn a_batch_merge_with_an_empty_fragment_stores_nothing_and_reports_success() {
    let (contended, id, version) = seeded().await;

    let result = batch_merge(&contended, &id, &empty_fragment()).await;

    assert_sole_success(&result, &id);
    assert_nothing_stored(&contended, &id, version);
}

/// Clause 5.6.20.4: a merge restating the stored value names an instance, whose
/// observation is recorded as it is for the single Merge Entity.
#[tokio::test]
async fn a_batch_merge_restating_the_stored_value_still_records_the_observation() {
    let (contended, id, _) = seeded().await;

    let result = batch_merge(&contended, &id, &identical_fragment()).await;

    assert_sole_success(&result, &id);
    assert_observation_recorded(&contended, &id);
}

/// A real merge still writes.
#[tokio::test]
async fn a_batch_merge_that_adds_an_attribute_still_writes() {
    let (contended, id, version) = seeded().await;

    let result = batch_merge(&contended, &id, &new_attribute_fragment()).await;

    assert_sole_success(&result, &id);
    assert_fresh_attribute_stored(&contended, &id, version);
}

/// Clause 5.6.8.4 update mode runs Update Attributes: an empty fragment on an
/// existing entity stores nothing, and the entity is reported as updated, not
/// created.
#[tokio::test]
async fn a_batch_upsert_update_with_an_empty_fragment_stores_nothing_and_reports_success() {
    let (contended, id, version) = seeded().await;

    let (result, created) = batch_upsert(&contended, &id, &empty_fragment(), UpsertPolicy::Update).await;

    assert_sole_success(&result, &id);
    assert!(created.is_empty(), "{created:?}");
    assert_nothing_stored(&contended, &id, version);
}

/// Clause 5.6.8.4 update mode: a fragment restating the stored value names an
/// instance, whose observation is recorded.
#[tokio::test]
async fn a_batch_upsert_update_restating_the_stored_value_still_records_the_observation() {
    let (contended, id, _) = seeded().await;

    let (result, created) = batch_upsert(&contended, &id, &identical_fragment(), UpsertPolicy::Update).await;

    assert_sole_success(&result, &id);
    assert!(created.is_empty(), "{created:?}");
    assert_observation_recorded(&contended, &id);
}

/// A real change in update mode still writes.
#[tokio::test]
async fn a_batch_upsert_update_that_adds_an_attribute_still_writes() {
    let (contended, id, version) = seeded().await;

    let (result, created) = batch_upsert(&contended, &id, &new_attribute_fragment(), UpsertPolicy::Update).await;

    assert_sole_success(&result, &id);
    assert!(created.is_empty(), "{created:?}");
    assert_eq!(contended.store.submitted_for(&id).len(), 1);
    assert!(contended.store.stored(&id).expect("the Building is stored").1 > version);
}

/// Clause 5.6.8.4 replace mode is the whole replacement of clause 5.6.18, not a
/// Fragment: restating the stored entity still writes.
#[tokio::test]
async fn a_batch_upsert_replace_restating_the_stored_entity_still_writes() {
    let (contended, id, _) = seeded().await;

    let (result, created) = batch_upsert(&contended, &id, &identical_fragment(), UpsertPolicy::Replace).await;

    assert_sole_success(&result, &id);
    assert!(created.is_empty(), "{created:?}");
    assert_eq!(contended.store.submitted_for(&id).len(), 1);
}
