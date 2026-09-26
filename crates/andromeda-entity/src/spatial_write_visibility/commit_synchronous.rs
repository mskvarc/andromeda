//! The `commit-synchronous` mode: every write path updates the index before it
//! returns.

use crate::{
    admission_fixture::{admitted_batch, admitted_entity},
    merge_options::MergeOptions,
    side_effect::BatchSideEffectMode,
    spatial_acceleration::SpatialAcceleration,
    spatial_write_visibility::{
        geo_query::{create, narrowed_ids, near_origin, uri, vehicle_at, warm_index},
        write_service::indexed_service,
    },
    write_kind::{BatchCreate, MergeEntity},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn under_commit_synchronous_a_created_entity_is_a_candidate_for_the_next_geo_query() {
    let index = warm_index();
    let service = indexed_service(&index);
    let query = near_origin("urn:ngsi-ld:Vehicle:new");
    // clone: the query service shares the index
    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(Arc::clone(&index)), &query).await,
        Some(Vec::new())
    );

    create(&service, vehicle_at("urn:ngsi-ld:Vehicle:new", 0.001)).await;

    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(index), &query).await,
        Some(vec![uri("urn:ngsi-ld:Vehicle:new")])
    );
}

#[tokio::test]
async fn under_commit_synchronous_a_merge_moving_an_entity_into_range_makes_it_a_candidate() {
    let index = warm_index();
    let service = indexed_service(&index);
    create(&service, vehicle_at("urn:ngsi-ld:Vehicle:mover", 50.0)).await;
    let query = near_origin("urn:ngsi-ld:Vehicle:mover");
    // clone: the query service shares the index
    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(Arc::clone(&index)), &query).await,
        Some(Vec::new())
    );

    let patch: EntityPatch =
        serde_json::from_value(json!({"location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.002, 0.0]}}})).expect("a valid patch");
    let admitted = admitted_entity::<MergeEntity>(&service, &patch, &ContextSpec::core_only()).await;
    service
        .merge(
            &ContextTarget::Live,
            &uri("urn:ngsi-ld:Vehicle:mover"),
            &patch,
            admitted,
            &ContextSpec::core_only(),
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");

    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(index), &query).await,
        Some(vec![uri("urn:ngsi-ld:Vehicle:mover")])
    );
}

#[tokio::test]
async fn under_commit_synchronous_a_deleted_entity_leaves_the_candidates() {
    let index = warm_index();
    let service = indexed_service(&index);
    create(&service, vehicle_at("urn:ngsi-ld:Vehicle:gone", 0.001)).await;
    service
        .delete(&ContextTarget::Live, &uri("urn:ngsi-ld:Vehicle:gone"))
        .await
        .expect("the delete lands");

    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(index), &near_origin("urn:ngsi-ld:Vehicle:gone")).await,
        Some(Vec::new())
    );
}

/// The batch's in-process events are deferred to the side-effect worker, which
/// may run long after the response; the index is not among the things it
/// defers.
#[tokio::test]
async fn under_commit_synchronous_an_enqueued_batch_create_is_a_candidate_before_its_side_effects_run() {
    let index = warm_index();
    let service = indexed_service(&index);
    let entity: Entity = serde_json::from_value(vehicle_at("urn:ngsi-ld:Vehicle:batched", 0.001)).expect("a valid entity");
    let admitted = admitted_batch(&service, BatchCreate, vec![entity], &ContextSpec::core_only()).await;
    service
        .batch_create(&ContextTarget::Live, admitted, &ContextSpec::core_only(), BatchSideEffectMode::Enqueue)
        .await
        .expect("the batch create lands");

    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(index), &near_origin("urn:ngsi-ld:Vehicle:batched")).await,
        Some(vec![uri("urn:ngsi-ld:Vehicle:batched")])
    );
}

/// Clause 5.7.2.4: any `location` instance meeting the restriction is enough.
/// The index holds the default instance alone, which here lies far outside the
/// area, so it finds no candidate -- and that narrows the default instance
/// only: the request's identifiers and the geo-query reach the store intact,
/// and the store evaluates the `datasetId` instance that does lie inside.
#[tokio::test]
async fn under_commit_synchronous_an_entity_matching_only_through_a_dataset_instance_is_not_narrowed_away() {
    let index = warm_index();
    let service = indexed_service(&index);
    create(
        &service,
        json!({
            "id": "urn:ngsi-ld:Vehicle:two-fixes",
            "type": "Vehicle",
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [50.0, 0.0]}},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.001, 0.0]}, "datasetId": "urn:ngsi-ld:Dataset:gps"}
            ]
        }),
    )
    .await;

    assert_eq!(
        narrowed_ids(SpatialAcceleration::CommitSynchronous(index), &near_origin("urn:ngsi-ld:Vehicle:two-fixes")).await,
        Some(Vec::new()),
        "the default instance is outside; the dataset instance is left to the store"
    );
}
