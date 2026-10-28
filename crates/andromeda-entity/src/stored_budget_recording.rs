//! The entity value limit holds for the entity a write stores, not only for
//! the document it writes: a merge or a batch merge each within the limit is
//! refused with 413 when it would grow the stored Building past it, nothing is
//! stored for it, and in a batch only that Entity is refused.

use crate::{
    admission_fixture::{admitted_batch, admitted_context, admitted_entity},
    configured_policy::payload_budget,
    error::EntityError,
    merge_options::MergeOptions,
    notification_recording_harness::{ENTITY_ID, budgeted_recording_service_with_writes, building_snapshot, core_context, entity_uri, live_target},
    side_effect::SideEffectPlacement,
    stored_census::stored_value_count,
    write_kind::{BatchMerge, MergeEntity},
    write_target::WriteTarget,
};
use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    attribute::merge_member::MergeMember,
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::payload_budget::{EntityPayloadBudget, PayloadBudgetExceeded};
use serde_json::{Value, json};
use std::num::NonZeroUsize;

/// The entity value limit every test runs under: 20 values above what the
/// stored Building holds.
fn tight_budget() -> EntityPayloadBudget {
    let held = stored_value_count(&building_snapshot()).get();
    EntityPayloadBudget {
        json_values: JsonValueLimit::new(NonZeroUsize::new(held + 20).expect("a positive value limit")),
        ..payload_budget(&RequestLimitsConfig::default())
    }
}

/// A `readings` Property of 20 numbers: 23 values once stored (the attribute,
/// its instance, its family and the numbers), within the limit as a document
/// of its own but past it added to the Building.
fn twenty_readings() -> Value {
    json!({"readings": {"type": "Property", "value": (0..20).collect::<Vec<i32>>()}})
}

/// A `name` Property: 4 values once stored.
fn a_name() -> Value {
    json!({"name": {"type": "Property", "value": "Tower"}})
}

/// A batch input for `id` carrying the compact members of `attributes`.
fn building(id: &str, attributes: &Value) -> Entity<MergeMember> {
    let mut entity = json!({"id": id, "type": "Building"});
    if let (Some(entity), Some(attributes)) = (entity.as_object_mut(), attributes.as_object()) {
        entity.extend(attributes.clone()); // clone: the fixture owns its members
    }
    serde_json::from_value(entity).expect("the fixture entity is valid")
}

/// Clause 6.3.2: a merge whose fragment fits the budget but whose result
/// would not is refused with 413, and nothing is stored.
#[tokio::test]
async fn a_merge_growing_the_stored_entity_past_the_value_limit_is_refused_and_stores_nothing() {
    let (service, recorded) = budgeted_recording_service_with_writes(tight_budget());
    let fragment: EntityPatch<MergeMember> = serde_json::from_value(twenty_readings()).expect("a merge fragment");
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;

    let refusal = service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect_err("the merged Building passes the limit");

    assert!(
        matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { .. })),
        "{refusal:?}"
    );
    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a refused merge stores nothing");
}

#[tokio::test]
async fn a_merge_keeping_the_stored_entity_within_the_value_limit_lands() {
    let (service, recorded) = budgeted_recording_service_with_writes(tight_budget());
    let fragment: EntityPatch<MergeMember> = serde_json::from_value(a_name()).expect("a merge fragment");
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
        .expect("the merged Building stays within the limit");

    pretty_assertions::assert_eq!(recorded.lock().expect("the recording buffer").len(), 1);
}

/// Clause 5.6.20.4 reports an error per Entity: the Entity the merge would
/// grow past the limit is refused with 413, the other one lands.
#[tokio::test]
async fn a_batch_merge_refuses_only_the_entity_it_would_grow_past_the_value_limit() {
    const OTHER_ID: &str = "urn:ngsi-ld:Building:within-budget";
    let (service, recorded) = budgeted_recording_service_with_writes(tight_budget());
    let admitted = admitted_batch(
        &service,
        BatchMerge,
        vec![building(ENTITY_ID, &twenty_readings()), building(OTHER_ID, &a_name())],
        &ContextSpec::core_only(),
    )
    .await;

    let result = service
        .batch_merge(SideEffectPlacement::Inline(&live_target()), admitted)
        .await
        .expect("the batch runs");

    let refused: Vec<&str> = result.errors.iter().map(|(id, _)| id.as_str()).collect();
    pretty_assertions::assert_eq!(refused, [ENTITY_ID]);
    assert!(
        result
            .errors
            .iter()
            .all(|(_, error)| matches!(error, EntityError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { .. }))),
        "{:?}",
        result.errors
    );
    let landed: Vec<&str> = result.success.iter().map(Identifier::as_str).collect();
    pretty_assertions::assert_eq!(landed, [OTHER_ID]);
    pretty_assertions::assert_eq!(recorded.lock().expect("the recording buffer").len(), 1);
}
