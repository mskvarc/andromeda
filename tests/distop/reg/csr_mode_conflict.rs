//! Mode-specific conflict and operations-subset rules, at registration and at
//! update (clauses 5.2.9, 5.9.3). Exclusive and redirect registrations may not
//! overlap another source or a local entity for the same id/attribute footprint,
//! and auxiliary registrations may only carry consumption operations. All
//! `T-LOCAL`.

use crate::{
    entity::vehicle,
    oracle::{assert_problem_details, assert_status},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, CONFLICT},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use serde_json::json;

/// Create a local `Vehicle` entity holding a `speed` property on the front broker.
async fn create_local_vehicle(topology: &MultiBrokerTopology<BrokerRole>, id: &str, speed: i64) {
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(id, speed))
        .with_ld_json()
        .send()
        .await
        .expect("create local entity");
    assert_status(&created, 201);
}

/// A second exclusive registration over the same id/attribute footprint as an
/// existing exclusive one is rejected (clause 5.2.9).
#[tokio::test]
async fn reg_exclusive_over_existing_exclusive_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:excl-excl");
    let first_id = topology.id("registration:excl-1");
    let second_id = topology.id("registration:excl-2");

    let first = CsrBuilder::new(&first_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&first)
        .await
        .expect("create first exclusive");
    assert_status(&created, 201);

    let second = CsrBuilder::new(&second_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&second)
        .await
        .expect("create overlapping exclusive");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration over the same footprint as an existing redirect
/// registration is rejected (clause 5.2.9).
#[tokio::test]
async fn reg_exclusive_over_existing_redirect_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:excl-redirect");
    let redirect_id = topology.id("registration:redirect-1");
    let exclusive_id = topology.id("registration:excl-over-redirect");

    let redirect = CsrBuilder::new(&redirect_id, PEER_ENDPOINT)
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&redirect)
        .await
        .expect("create redirect");
    assert_status(&created, 201);

    let exclusive = CsrBuilder::new(&exclusive_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&exclusive)
        .await
        .expect("create overlapping exclusive");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration over a local entity that already holds the covered
/// attribute is rejected (clause 5.2.9).
#[tokio::test]
async fn reg_exclusive_over_local_entity_with_attr_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:excl-local");
    let registration_id = topology.id("registration:excl-local");
    create_local_vehicle(&topology, &entity_id, 50).await;

    let exclusive = CsrBuilder::new(&registration_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&exclusive)
        .await
        .expect("create exclusive over local entity");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration over an entity that already exists locally is rejected
/// (clause 5.2.9).
#[tokio::test]
async fn reg_redirect_over_existing_entity_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:redirect-local");
    let registration_id = topology.id("registration:redirect-local");
    create_local_vehicle(&topology, &entity_id, 50).await;

    let redirect = CsrBuilder::new(&registration_id, PEER_ENDPOINT).mode("redirect").entity_id(&entity_id).build();
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&redirect)
        .await
        .expect("create redirect over local entity");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration may not advertise a write operation (clause 5.2.9).
#[tokio::test]
async fn reg_auxiliary_with_write_operation_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:aux-write");

    let auxiliary = CsrBuilder::new(&registration_id, PEER_ENDPOINT)
        .mode("auxiliary")
        .entity_type(&vtype)
        .operations(&["createEntity"])
        .build();
    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&auxiliary)
        .await
        .expect("create auxiliary with write op");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// An auxiliary registration carrying only consumption operations is accepted
/// (clause 5.2.9).
#[tokio::test]
async fn reg_auxiliary_with_valid_consumption_ops_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:aux-consume");

    let auxiliary = CsrBuilder::new(&registration_id, PEER_ENDPOINT)
        .mode("auxiliary")
        .entity_type(&vtype)
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&auxiliary)
        .await
        .expect("create auxiliary with consumption ops");
    assert_status(&created, 201);

    topology.finish().await.expect("topology cleanup");
}

/// Updating one exclusive registration so it overlaps another exclusive one is
/// rejected (clause 5.9.3).
#[tokio::test]
async fn update_producing_exclusive_overlap_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-overlap");
    let first_id = topology.id("registration:update-overlap-a");
    let second_id = topology.id("registration:update-overlap-b");

    let first = CsrBuilder::new(&first_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&first)
            .await
            .expect("create exclusive A"),
        201,
    );
    let second = CsrBuilder::new(&second_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&second)
            .await
            .expect("create exclusive B"),
        201,
    );

    let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle", "id": entity_id } ], "propertyNames": ["fuel", "speed"] } ] });
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_registration(&second_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("patch B to overlap A");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// Updating an exclusive registration so it covers an attribute a local entity
/// already holds is rejected (clause 5.9.3).
#[tokio::test]
async fn update_exclusive_onto_existing_entity_attr_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-onto-local");
    let registration_id = topology.id("registration:update-onto-local");
    create_local_vehicle(&topology, &entity_id, 50).await;

    let exclusive = CsrBuilder::new(&registration_id, PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["fuel"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&exclusive)
            .await
            .expect("create exclusive B"),
        201,
    );

    let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle", "id": entity_id } ], "propertyNames": ["fuel", "speed"] } ] });
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_registration(&registration_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("patch exclusive onto local attribute");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// Updating a redirect registration so it matches an existing local entity is
/// rejected (clause 5.9.3).
#[tokio::test]
async fn update_redirect_matching_existing_entity_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let local_id = topology.id("Vehicle:update-redirect-local");
    let other_id = topology.id("Vehicle:update-redirect-other");
    let registration_id = topology.id("registration:update-redirect");
    create_local_vehicle(&topology, &local_id, 50).await;

    let redirect = CsrBuilder::new(&registration_id, PEER_ENDPOINT).mode("redirect").entity_id(&other_id).build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&redirect)
            .await
            .expect("create redirect for other id"),
        201,
    );

    let fragment = json!({ "information": [ { "entities": [ { "type": "Vehicle", "id": local_id } ] } ] });
    let conflict = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_registration(&registration_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("patch redirect onto local entity");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// Updating an auxiliary registration to advertise a write operation is rejected
/// (clause 5.9.3).
#[tokio::test]
async fn update_auxiliary_with_write_operation_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:update-aux-write");

    let auxiliary = CsrBuilder::new(&registration_id, PEER_ENDPOINT)
        .mode("auxiliary")
        .entity_type(&vtype)
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&auxiliary)
            .await
            .expect("create auxiliary"),
        201,
    );

    let fragment = json!({ "operations": ["updateEntity"] });
    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_registration(&registration_id, &fragment, None)
        .await
        .expect("patch auxiliary to write op");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}
