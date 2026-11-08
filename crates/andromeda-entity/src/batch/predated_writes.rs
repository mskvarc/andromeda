//! A batch write that reaches an entity another writer stamped later than the
//! batch, while the batch ran, is written after that write (clause 4.8: the
//! last modification never carries an instant earlier than one it replaced),
//! as a separate request would write it (clause 5.5.11.0). Every other entity
//! of the batch keeps the one batch instant.
//!
//! Each test lands a write on the Building `a` after the batch read its clock
//! and before it read or wrote `a`, as another replica's write would, beside
//! the Building `b` no other write touches. A clock that stepped back behind
//! the instant an entity holds is answered the same way: the write lands one
//! microsecond after that instant.

use crate::{
    admission_fixture::{admitted_batch, admitted_written},
    admitted_write::AdmittedEntity,
    batch::{BatchResult, UpsertPolicy},
    compare_and_set_fixture::{
        ContendedService,
        Watchers,
        attribute_adder,
        batch_merge,
        building_id,
        contended_service,
        request_building,
        stamped_modified_at,
        stored_building,
        upsert_update,
    },
    in_memory_rows::StoredRows,
    in_memory_store::ConcurrentWriter,
    side_effect::SideEffectPlacement,
    write_kind::{AppendAttributes, BatchCreate, BatchUpdate, BatchUpsert, CreateEntity},
    write_target::WriteTarget,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use chrono::{DateTime, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The branch every test writes.
const LIVE: ContextTarget = ContextTarget::Live;

/// The instant another writer landed its write on `a`, once it has run.
type Landed = Arc<Mutex<Option<DateTime<Utc>>>>;

/// The attributes the issue's upsert writes: a new `level` and `alarm`
/// "off", each a change of what the Buildings hold.
fn level_and_alarm() -> Value {
    json!({"level": {"type": "Property", "value": 7}, "alarm": {"type": "Property", "value": "off"}})
}

/// The microsecond the store keeps of `seconds` from now, later when
/// positive.
fn from_now(seconds: i64) -> DateTime<Utc> {
    SystemInstant::new(Utc::now() + TimeDelta::seconds(seconds)).get()
}

/// One microsecond past `instant`.
fn just_after(instant: DateTime<Utc>) -> DateTime<Utc> {
    instant + TimeDelta::microseconds(1)
}

/// A service holding the Buildings `a` and `b`, each with `level`, stamped a
/// minute ago.
async fn two_buildings() -> (ContendedService, Identifier, Identifier) {
    let contended = contended_service(Watchers::None).await;
    let (a, b) = (building_id("a"), building_id("b"));
    contended.store.seed_at(&a, stored_building(&a, &["level"]), from_now(-60));
    contended.store.seed_at(&b, stored_building(&b, &["level"]), from_now(-60));
    (contended, a, b)
}

/// Have another writer add `alarm` to `id` after the batch read its clock and
/// before it reads `id`, answering when that write landed.
fn another_writer_appends_before_the_read(contended: &ContendedService, id: &Identifier) -> Landed {
    let landed: Landed = Arc::new(Mutex::new(None));
    let recorded = Arc::clone(&landed);
    let touched = id.clone();
    contended.store.interfere_before_read(attribute_adder(id, "alarm", move |rows: &StoredRows| {
        *recorded.lock().expect("the instant lock is not poisoned") = rows.modified_at(&touched);
    }));
    landed
}

/// `ids` in identifier order: an entity written again settles after the
/// entities whose first attempt landed.
fn by_identifier(mut ids: Vec<Identifier>) -> Vec<Identifier> {
    ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    ids
}

/// The instant the other writer landed its write at.
fn landed_at(landed: &Landed) -> DateTime<Utc> {
    landed.lock().expect("the instant lock is not poisoned").expect("the other writer ran")
}

/// `a`'s write was refused at the batch instant and landed after the other
/// write, while `b` holds the batch instant `a`'s first attempt stamped, and
/// every instance `a` holds was modified no earlier than it was created.
fn assert_written_after_the_other_write(contended: &ContendedService, a: &Identifier, b: &Identifier, landed: DateTime<Utc>) {
    let writes = contended.store.submitted_for(a);
    assert_eq!(writes.len(), 2, "the predated attempt and the one stamped after the other write");
    let batch = stamped_modified_at(&writes[0], "level").expect("the batch stamps level").get();
    assert!(batch < landed, "the batch read its clock at {batch}, before the other write at {landed}");
    let rewritten = contended.store.modified_at(a).expect("a is stored");
    assert!(rewritten > landed, "a is modified at {rewritten}, after the other write at {landed}");
    assert_eq!(stamped_modified_at(&writes[1], "level").map(SystemInstant::get), Some(rewritten));
    assert_eq!(contended.store.modified_at(b), Some(batch), "b keeps the batch instant");
    let (state, _) = contended.store.stored(a).expect("a is stored");
    for (name, instances) in state.attributes().iter() {
        for (_, instance) in instances.iter() {
            assert!(instance.modified_at() >= instance.created_at(), "{name} was modified before it was created");
        }
    }
}

/// The issue's own request: the upsert in update mode reads `a` once the
/// append landed, and is stamped after it rather than at its own earlier
/// instant.
#[tokio::test]
async fn a_batch_upsert_update_reaching_an_entity_after_a_later_append_writes_it_after_the_append() {
    let (contended, a, b) = two_buildings().await;
    let landed = another_writer_appends_before_the_read(&contended, &a);

    let (result, created) = upsert_update(
        &contended,
        vec![request_building(&a, &level_and_alarm()), request_building(&b, &level_and_alarm())],
    )
    .await;

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(created.is_empty(), "{created:?}");
    assert_written_after_the_other_write(&contended, &a, &b, landed_at(&landed));
}

#[tokio::test]
async fn a_batch_upsert_replace_reaching_an_entity_after_a_later_append_writes_it_after_the_append() {
    let (contended, a, b) = two_buildings().await;
    let landed = another_writer_appends_before_the_read(&contended, &a);

    let admitted = admitted_batch(
        &contended.service,
        BatchUpsert(UpsertPolicy::Replace),
        vec![request_building(&a, &level_and_alarm()), request_building(&b, &level_and_alarm())],
        &ContextSpec::core_only(),
    )
    .await;
    let (result, _) = contended
        .service
        .batch_upsert(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes");

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert_written_after_the_other_write(&contended, &a, &b, landed_at(&landed));
}

#[tokio::test]
async fn a_batch_update_reaching_an_entity_after_a_later_append_writes_it_after_the_append() {
    let (contended, a, b) = two_buildings().await;
    let landed = another_writer_appends_before_the_read(&contended, &a);

    let admitted = admitted_batch(
        &contended.service,
        BatchUpdate,
        vec![request_building(&a, &level_and_alarm()), request_building(&b, &level_and_alarm())],
        &ContextSpec::core_only(),
    )
    .await;
    let result = contended
        .service
        .batch_update(SideEffectPlacement::Inline(&LIVE), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the batch completes");

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert_written_after_the_other_write(&contended, &a, &b, landed_at(&landed));
}

#[tokio::test]
async fn a_batch_merge_reaching_an_entity_after_a_later_append_writes_it_after_the_append() {
    let (contended, a, b) = two_buildings().await;
    let landed = another_writer_appends_before_the_read(&contended, &a);

    let result = batch_merge(
        &contended,
        vec![request_building(&a, &level_and_alarm()), request_building(&b, &level_and_alarm())],
    )
    .await;

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert_written_after_the_other_write(&contended, &a, &b, landed_at(&landed));
}

/// Clause 5.6.6 records the deletion's tombstones at its instant, so a
/// deletion that reaches `a` after a later write records them after that
/// write's observations, and the Temporal Evolution shows no instance of `a`
/// in effect once it is gone.
#[tokio::test]
async fn a_batch_delete_reaching_an_entity_after_a_later_write_records_its_deletion_after_it() {
    let (contended, a, b) = two_buildings().await;
    let landed = another_writer_appends_before_the_read(&contended, &a);

    let result = contended
        .service
        .batch_delete(SideEffectPlacement::Inline(&LIVE), &[a.clone(), b.clone()])
        .await
        .expect("the batch completes");

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    let landed = landed_at(&landed);
    assert_eq!(
        contended.store.submitted_for(&a).len(),
        2,
        "the predated deletion and the one after the other write"
    );
    let deleted_a = contended.store.evolution_at(&a).expect("a's Evolution stands");
    let deleted_b = contended.store.evolution_at(&b).expect("b's Evolution stands");
    assert!(deleted_a > landed, "a is deleted at {deleted_a}, after the other write at {landed}");
    assert!(deleted_b < landed, "b is deleted at the batch instant {deleted_b}");
    assert!(contended.store.stored(&a).is_none());
}

/// Create `entities` in one batch, inline.
async fn batch_create(contended: &ContendedService, entities: &[&Identifier]) -> BatchResult {
    let requests = entities.iter().map(|id| request_building(id, &level_and_alarm())).collect();
    let admitted = admitted_batch(&contended.service, BatchCreate, requests, &ContextSpec::core_only()).await;
    contended
        .service
        .batch_create(SideEffectPlacement::Inline(&LIVE), admitted)
        .await
        .expect("the batch completes")
}

/// A core deletion of `a` commits while the batch runs, after it read its
/// clock: clause 5.6.1 creates `a` anew, after the tombstones the deletion
/// recorded rather than before them.
#[tokio::test]
async fn a_batch_create_of_an_identifier_deleted_while_it_ran_records_its_creation_after_the_deletion() {
    let contended = contended_service(Watchers::None).await;
    let (a, b) = (building_id("a"), building_id("b"));
    contended.store.seed_at(&a, stored_building(&a, &["level"]), from_now(-60));
    let deleted: Landed = Arc::new(Mutex::new(None));
    let (recorded, retired) = (Arc::clone(&deleted), a.clone());
    contended
        .store
        .interfere(ConcurrentWriter::BeforeNextCommit(Box::new(move |rows: &mut StoredRows| {
            let at = SystemInstant::new(Utc::now()).get();
            rows.retire(&retired, at);
            *recorded.lock().expect("the instant lock is not poisoned") = Some(at);
        })));

    let result = batch_create(&contended, &[&a, &b]).await;

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let deleted = landed_at(&deleted);
    let writes = contended.store.submitted_for(&a);
    assert_eq!(writes.len(), 2, "the predated creation and the one after the deletion");
    let batch = stamped_modified_at(&writes[0], "level").expect("the batch stamps level").get();
    let created = contended.store.modified_at(&a).expect("a is created anew");
    assert!(created > deleted, "a is created at {created}, after the deletion at {deleted}");
    assert_eq!(stamped_modified_at(&writes[1], "level").map(SystemInstant::get), Some(created));
    assert_eq!(contended.store.modified_at(&b), Some(batch), "b keeps the batch instant");
}

/// An identifier that exists when the create commits is a conflict no later
/// attempt can resolve, answered once (NGSI-LD 6.4.3.1).
#[tokio::test]
async fn a_batch_create_of_an_existing_identifier_is_answered_as_a_conflict_without_a_retry() {
    let contended = contended_service(Watchers::None).await;
    let a = building_id("a");
    contended.store.seed_at(&a, stored_building(&a, &["level"]), from_now(-60));

    let result = batch_create(&contended, &[&a]).await;

    assert!(result.success.is_empty(), "{:?}", result.success);
    assert!(
        matches!(result.errors.as_slice(), [(failed, error)] if *failed == a && !error.is_not_found()),
        "{:?}",
        result.errors
    );
    assert_eq!(contended.store.submitted_for(&a).len(), 1);
}

/// The clock stepped back behind the instant `a` holds: `a` lands one
/// microsecond after it, and `b` keeps the batch instant.
#[tokio::test]
async fn a_batch_merge_after_the_clock_stepped_back_writes_the_entity_one_microsecond_after_its_instant() {
    let contended = contended_service(Watchers::None).await;
    let (a, b) = (building_id("a"), building_id("b"));
    let ahead = from_now(3600);
    contended.store.seed_at(&a, stored_building(&a, &["level"]), ahead);
    contended.store.seed_at(&b, stored_building(&b, &["level"]), from_now(-60));

    let result = batch_merge(
        &contended,
        vec![request_building(&a, &level_and_alarm()), request_building(&b, &level_and_alarm())],
    )
    .await;

    assert_eq!(by_identifier(result.success), vec![a.clone(), b.clone()]);
    assert_eq!(contended.store.modified_at(&a), Some(just_after(ahead)));
    assert!(contended.store.modified_at(&b).is_some_and(|batch| batch < ahead));
}

/// The admitted creation of the Building `id` holding `level` and `alarm`.
async fn admitted_creation(contended: &ContendedService, id: &Identifier) -> AdmittedEntity<CreateEntity> {
    let mut payload = level_and_alarm();
    payload["id"] = json!(id.as_str());
    payload["type"] = json!("Building");
    admitted_written::<CreateEntity>(&contended.service, payload, &ContextSpec::core_only()).await
}

/// A core deletion of `a` commits after the create read its clock and
/// before the create committed: clause 5.6.1 creates `a` anew after the
/// deletion's tombstones.
#[tokio::test]
async fn a_create_of_an_identifier_deleted_after_it_read_its_clock_lands_after_the_deletion() {
    let contended = contended_service(Watchers::None).await;
    let a = building_id("a");
    contended.store.seed_at(&a, stored_building(&a, &["level"]), from_now(-60));
    let deleted: Landed = Arc::new(Mutex::new(None));
    let (recorded, retired) = (Arc::clone(&deleted), a.clone());
    contended
        .store
        .interfere(ConcurrentWriter::BeforeNextCommit(Box::new(move |rows: &mut StoredRows| {
            let at = from_now(1);
            rows.retire(&retired, at);
            *recorded.lock().expect("the instant lock is not poisoned") = Some(at);
        })));

    let admitted = admitted_creation(&contended, &a).await;
    contended.service.create(&LIVE, &a, admitted).await.expect("the create lands");

    let deleted = landed_at(&deleted);
    let created = contended.store.modified_at(&a).expect("a is created anew");
    assert!(created > deleted, "a is created at {created}, after the deletion at {deleted}");
    assert_eq!(contended.store.submitted_for(&a).len(), 2);
}

/// Clause 5.6.1 creates an Entity anew on an identifier whose Temporal
/// Evolution stands at a later instant than the clock reads: it lands one
/// microsecond after that instant.
#[tokio::test]
async fn a_create_after_the_clock_stepped_back_lands_one_microsecond_after_the_evolution() {
    let contended = contended_service(Watchers::None).await;
    let a = building_id("a");
    let ahead = from_now(3600);
    contended.store.seed_evolution(&a, ahead);

    let admitted = admitted_creation(&contended, &a).await;
    contended.service.create(&LIVE, &a, admitted).await.expect("the create lands");

    assert_eq!(contended.store.modified_at(&a), Some(just_after(ahead)));
    let writes = contended.store.submitted_for(&a);
    assert_eq!(writes.len(), 2, "the predated create and the one after the Evolution");
    assert_eq!(stamped_modified_at(&writes[1], "level").map(SystemInstant::get), Some(just_after(ahead)));
}

/// A single read-modify-write is refused only once the clock stepped back
/// behind the entity's `modifiedAt`, and lands one microsecond after it.
#[tokio::test]
async fn an_append_after_the_clock_stepped_back_lands_one_microsecond_after_the_entity_s_instant() {
    let contended = contended_service(Watchers::None).await;
    let a = building_id("a");
    let ahead = from_now(3600);
    contended.store.seed_at(&a, stored_building(&a, &["level"]), ahead);

    let admitted = admitted_written::<AppendAttributes>(&contended.service, level_and_alarm(), &ContextSpec::core_only()).await;
    contended
        .service
        .append_attrs(&LIVE, WriteTarget::new(&a, None), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the append lands");

    assert_eq!(contended.store.modified_at(&a), Some(just_after(ahead)));
    assert_eq!(contended.store.submitted_for(&a).len(), 2);
}

/// A deletion whose clock stepped back behind the entity's `modifiedAt`
/// records its tombstones one microsecond after it.
#[tokio::test]
async fn a_delete_after_the_clock_stepped_back_records_its_deletion_one_microsecond_after_the_entity_s_instant() {
    let contended = contended_service(Watchers::None).await;
    let a = building_id("a");
    let ahead = from_now(3600);
    contended.store.seed_at(&a, stored_building(&a, &["level"]), ahead);

    contended.service.delete(&LIVE, WriteTarget::new(&a, None)).await.expect("the deletion lands");

    assert_eq!(contended.store.evolution_at(&a), Some(just_after(ahead)));
    assert!(contended.store.stored(&a).is_none());
}
