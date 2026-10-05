//! Every batch write computed from a state it read holds against that state's
//! version, and is rebuilt from a fresh read when another writer got there
//! first (clause 5.5.11.0: the entities are treated "as if they had all
//! arrived in separate requests").
//!
//! Each test changes a stored Building between the batch's read and its
//! commit, as another broker replica would, and reads off the store which
//! writes were submitted, what they held against, and what the Building holds
//! once the batch answered.

use crate::{
    admission_fixture::admitted_batch,
    batch::{BatchResult, UpsertPolicy},
    compare_and_set_fixture::{
        ContendedService,
        Watchers,
        adds_attribute,
        attribute_terms,
        building_id,
        contended_service,
        held_version,
        request_building,
        stored_building,
        term,
        tombstoned_terms,
        written_terms,
    },
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    in_memory_store::{ConcurrentWriter, StoredRows},
    side_effect::SideEffectPlacement,
    write_kind::{BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_context_commit::{atomic_write::AtomicWrite, precondition::Precondition, write_retry::WRITE_ATTEMPTS};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{instance::Instance, operation::selection::OverwritePolicy, system_instant::SystemInstant};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::Entity};
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    slice::from_ref,
    sync::{Arc, Mutex},
};

/// The branch every test writes.
const LIVE: ContextTarget = ContextTarget::Live;

/// The one Property `name` of a compact request entity.
fn property(name: &str) -> Value {
    json!({name: {"type": "Property", "value": 3}})
}

/// A service whose store holds the Building `a` with `seed`, and the version
/// it holds it at.
async fn seeded() -> (ContendedService, Identifier, DateTime<Utc>) {
    let contended = contended_service(Watchers::None).await;
    let id = building_id("a");
    contended.store.seed(&id, stored_building(&id, &["seed"]));
    let (_, version) = contended.store.stored(&id).expect("the Building is stored");
    (contended, id, version)
}

/// Have another writer add `other` to the Building `id` before the next
/// commit, answering the version it leaves the Building at once it has run.
fn another_writer_adds_other(contended: &ContendedService, id: &Identifier) -> Arc<Mutex<Option<DateTime<Utc>>>> {
    let version = Arc::new(Mutex::new(None));
    contended.store.interfere(adds_attribute(id, "other", &version));
    version
}

/// The version the concurrent writer left behind.
fn concurrent_version(version: &Arc<Mutex<Option<DateTime<Utc>>>>) -> DateTime<Utc> {
    version.lock().expect("the version lock is not poisoned").expect("the concurrent writer ran")
}

/// The attribute terms the Building `id` holds once the batch answered.
fn stored_terms(contended: &ContendedService, id: &Identifier) -> Vec<String> {
    attribute_terms(&contended.store.stored(id).expect("the Building is stored").0)
}

/// The terms `expected` names, as [`attribute_terms`] lists them.
fn terms(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|name| (*name).to_owned()).collect()
}

/// Merge `entities` in one batch, inline.
async fn batch_merge(contended: &ContendedService, entities: Vec<Entity<MergeMember>>) -> BatchResult {
    let admitted = admitted_batch(&contended.service, BatchMerge, entities, &ContextSpec::core_only()).await;
    contended
        .service
        .batch_merge(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes")
}

/// Clause 5.6.20.4 runs Merge Entity per entity: the merge another writer's
/// change superseded is recomputed against the Building that change left, so
/// both changes stand, and only the landed attempt is notified.
#[tokio::test]
async fn a_batch_merge_superseded_by_another_writer_is_recomputed_against_its_change() {
    let (contended, id, read_version) = seeded().await;
    let concurrent = another_writer_adds_other(&contended, &id);

    let result = batch_merge(&contended, vec![request_building(&id, &property("m"))]).await;

    assert_eq!(result.success, vec![id.clone()]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2, "the superseded attempt and the one that landed");
    assert_eq!(held_version(&writes[0]), read_version);
    assert_eq!(held_version(&writes[1]), concurrent_version(&concurrent));
    assert_eq!(written_terms(&writes[1]), terms(&["m", "other", "seed"]));
    assert_eq!(stored_terms(&contended, &id), terms(&["m", "other", "seed"]));
    assert_eq!(
        contended.published.lock().expect("recording mutex is not poisoned").len(),
        1,
        "only the landed attempt notifies"
    );
}

/// Clause 5.6.9.4 runs the update per entity: the concurrent change survives.
#[tokio::test]
async fn a_batch_update_superseded_by_another_writer_keeps_its_change() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    let admitted = admitted_batch(
        &contended.service,
        BatchUpdate,
        vec![request_building(&id, &property("u"))],
        &ContextSpec::core_only(),
    )
    .await;
    let result = contended
        .service
        .batch_update(SideEffectPlacement::Inline(&LIVE), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the batch completes");

    assert_eq!(result.success, vec![id.clone()]);
    assert_eq!(contended.store.submitted_for(&id).len(), 2);
    assert_eq!(stored_terms(&contended, &id), terms(&["other", "seed", "u"]));
}

/// Clause 5.6.9.5: an append that adds nothing leaves the Entity out of S.
/// Whether it added anything is decided by the state the landed attempt read,
/// and the error is reported once, not once per attempt.
#[tokio::test]
async fn a_no_overwrite_conflict_is_reported_once_however_many_attempts_ran() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    let admitted = admitted_batch(
        &contended.service,
        BatchUpdate,
        vec![request_building(&id, &property("seed"))],
        &ContextSpec::core_only(),
    )
    .await;
    let result = contended
        .service
        .batch_update(SideEffectPlacement::Inline(&LIVE), admitted, OverwritePolicy::Preserve)
        .await
        .expect("the batch completes");

    assert_eq!(contended.store.submitted_for(&id).len(), 2);
    let conflicts: Vec<&EntityError> = result.errors.iter().filter(|(failed, _)| *failed == id).map(|(_, error)| error).collect();
    assert!(
        matches!(conflicts.as_slice(), [EntityError::Validation(EntityValidationReason::NoOverwriteConflict)]),
        "{conflicts:?}"
    );
}

/// Upsert the Building `id` with `attributes` in update mode, inline.
async fn upsert_update(contended: &ContendedService, id: &Identifier, attributes: &Value) -> (BatchResult, Vec<Identifier>) {
    let admitted = admitted_batch(
        &contended.service,
        BatchUpsert(UpsertPolicy::Update),
        vec![request_building(id, attributes)],
        &ContextSpec::core_only(),
    )
    .await;
    contended
        .service
        .batch_upsert(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes")
}

/// Clause 5.6.8.4 update mode runs clause 5.6.2 on an Entity that exists.
#[tokio::test]
async fn an_upsert_update_superseded_by_another_writer_keeps_its_change() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    let (result, created) = upsert_update(&contended, &id, &property("u")).await;

    assert_eq!(result.success, vec![id.clone()]);
    assert_eq!(created, Vec::<Identifier>::new());
    assert_eq!(contended.store.submitted_for(&id).len(), 2);
    assert_eq!(stored_terms(&contended, &id), terms(&["other", "seed", "u"]));
}

/// Clause 5.6.8.4 update mode creates an Entity that does not exist and
/// updates one that does. When a concurrent create lands between the upsert's
/// read and its insert, the Entity exists after all, and the upsert updates
/// it rather than answering success with its content dropped.
#[tokio::test]
async fn an_upsert_update_that_loses_the_create_race_updates_the_created_entity() {
    let contended = contended_service(Watchers::None).await;
    let id = building_id("a");
    let concurrent = another_writer_adds_other(&contended, &id);

    let (result, created) = upsert_update(&contended, &id, &property("u")).await;

    assert_eq!(result.success, vec![id.clone()]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(created.is_empty(), "the Entity was created by the other request");
    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].precondition, Precondition::MustNotExist);
    assert_eq!(held_version(&writes[1]), concurrent_version(&concurrent));
    assert_eq!(stored_terms(&contended, &id), terms(&["other", "u"]));
}

/// Clause 5.6.8.4 creates an Entity it finds absent by clause 5.6.1, which
/// supports no NGSI-LD Null (clause 5.2.1): the entity is refused on what the
/// upsert read, before any write is submitted.
#[tokio::test]
async fn an_upsert_update_refuses_a_null_on_the_create_branch_before_writing() {
    let contended = contended_service(Watchers::None).await;
    let id = building_id("a");
    another_writer_adds_other(&contended, &id);

    let attributes = json!({
        "u": {"type": "Property", "value": 3},
        "other": {"type": "Property", "value": "urn:ngsi-ld:null"}
    });
    let (result, created) = upsert_update(&contended, &id, &attributes).await;

    assert!(result.success.is_empty(), "{:?}", result.success);
    assert!(created.is_empty(), "{created:?}");
    assert!(
        matches!(result.errors.as_slice(), [(failed, error)] if *failed == id && error.is_validation()),
        "{:?}",
        result.errors
    );
    assert!(contended.store.submitted_for(&id).is_empty());
}

/// Clause 5.6.8.4 replace mode replaces the Entity "completely": the
/// replacement superseded by another writer is recomputed, and its tombstones
/// end the instance the other writer added along with the ones it read.
#[tokio::test]
async fn an_upsert_replace_superseded_by_another_writer_ends_the_instance_it_added() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    let admitted = admitted_batch(
        &contended.service,
        BatchUpsert(UpsertPolicy::Replace),
        vec![request_building(&id, &property("r"))],
        &ContextSpec::core_only(),
    )
    .await;
    let (result, _) = contended
        .service
        .batch_upsert(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes");

    assert_eq!(result.success, vec![id.clone()]);
    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2);
    assert_eq!(tombstoned_terms(&writes[0]), terms(&["seed"]));
    assert_eq!(tombstoned_terms(&writes[1]), terms(&["other", "seed"]));
    assert_eq!(stored_terms(&contended, &id), terms(&["r"]));
}

/// An entity that loses its precondition on every attempt is reported as a
/// conflict (Table 6.3.2-1) once the budget is spent, while the entity beside
/// it in the chunk lands on its first attempt.
#[tokio::test]
async fn an_entity_superseded_on_every_attempt_reports_contention_and_spares_its_neighbour() {
    let contended = contended_service(Watchers::None).await;
    let contested = building_id("a");
    let quiet = building_id("b");
    contended.store.seed(&contested, stored_building(&contested, &["seed"]));
    contended.store.seed(&quiet, stored_building(&quiet, &["seed"]));
    let touched = contested.clone(); // clone: the writer outlives the borrow
    contended
        .store
        .interfere(ConcurrentWriter::BeforeEveryCommit(Box::new(move |rows: &mut StoredRows| {
            let payload = rows.payload(&touched).cloned().expect("the contested Building is stored"); // clone: the writer stores the same payload again
            rows.put(&touched, payload);
        })));

    let result = batch_merge(
        &contended,
        vec![request_building(&contested, &property("m")), request_building(&quiet, &property("m"))],
    )
    .await;

    assert_eq!(result.success, vec![quiet.clone()]);
    assert!(
        matches!(result.errors.as_slice(), [(failed, EntityError::Contention { .. })] if *failed == contested),
        "{:?}",
        result.errors
    );
    assert_eq!(
        contended.store.submitted_for(&contested).len(),
        usize::try_from(WRITE_ATTEMPTS).expect("the budget fits")
    );
    assert_eq!(contended.store.submitted_for(&quiet).len(), 1);
}

/// An entity deleted between the read and the commit is answered as the
/// absent Entity it now is (clause 5.6.20.4 runs clause 5.6.17, which
/// answers `ResourceNotFound`), not merged back into existence.
#[tokio::test]
async fn an_entity_deleted_by_another_writer_is_reported_not_found() {
    let (contended, id, _) = seeded().await;
    let deleted = id.clone(); // clone: the writer outlives the borrow
    contended
        .store
        .interfere(ConcurrentWriter::BeforeNextCommit(Box::new(move |rows: &mut StoredRows| rows.remove(&deleted))));

    let result = batch_merge(&contended, vec![request_building(&id, &property("m"))]).await;

    assert_eq!(result.success, Vec::<Identifier>::new());
    assert!(
        matches!(result.errors.as_slice(), [(failed, error)] if *failed == id && error.is_not_found()),
        "{:?}",
        result.errors
    );
    assert_eq!(contended.store.submitted_for(&id).len(), 1, "the re-read finds nothing to write");
    assert!(contended.store.stored(&id).is_none());
}

/// Clause 5.6.10.4 runs Delete Entity per entity, which ends every instance
/// the entity held when it was deleted: the one another writer added after the
/// read is ended too.
#[tokio::test]
async fn a_batch_delete_superseded_by_another_writer_ends_the_instance_it_added() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    let result = contended
        .service
        .batch_delete(SideEffectPlacement::Inline(&LIVE), from_ref(&id))
        .await
        .expect("the batch completes");

    assert_eq!(result.success, vec![id.clone()]);
    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2);
    assert_eq!(tombstoned_terms(&writes[1]), terms(&["other", "seed"]));
    assert!(contended.store.stored(&id).is_none());
}

/// The `modifiedAt` the write stamped on the instance of `attribute`.
fn stamped_modified_at(write: &AtomicWrite, attribute: &str) -> Option<SystemInstant> {
    let state = &write.entity.record().expect("the write stores a state").state;
    state
        .attributes()
        .get(&term(attribute))
        .and_then(|instances| instances.get(None))
        .and_then(Instance::modified_at)
}

/// Clause 5.5.11.0: "All Entities and Attributes in the batch will get the
/// same modifiedAt timestamp", so a retried attempt stamps the instant the
/// first one did.
#[tokio::test]
async fn a_retried_attempt_stamps_the_batch_instant_the_first_attempt_stamped() {
    let (contended, id, _) = seeded().await;
    another_writer_adds_other(&contended, &id);

    batch_merge(&contended, vec![request_building(&id, &property("m"))]).await;

    let writes = contended.store.submitted_for(&id);
    assert_eq!(writes.len(), 2);
    let first = stamped_modified_at(&writes[0], "m");
    assert!(first.is_some(), "the merged instance is stamped");
    assert_eq!(stamped_modified_at(&writes[1], "m"), first);
}
