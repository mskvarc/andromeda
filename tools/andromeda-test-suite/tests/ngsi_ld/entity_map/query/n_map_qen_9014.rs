//! A follow-up under a map may name other identifiers.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities under an `EntityMap` that Query Entities with `entityMap=true` created by a type alone, with
/// `id` naming one mapped Entity answers that Entity, and with `id` naming two others answers those two
/// Clause: 5.5.14, 5.7.2.3, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_9014_a_follow_up_under_a_map_may_name_other_identifiers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_ids = [namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?, namespace.urn("Vehicle", 3)?];
    let mut vehicles = Vec::new();
    for id in &vehicle_ids {
        vehicles.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(vehicle_type.clone())
                .property("speed", 50)
                .build()?,
        );
    }
    broker.entities().batch_create(&vehicles).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json
        .with_heads()
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&EntityMapRequest::returned())
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let reuse = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let one = json
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_ids[0].clone()])
        .entity_map(&reuse)
        .send()
        .await?;
    expect_that!(one.items(), elements_are![has_id(&vehicle_ids[0])]);
    let two = json
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_ids[1].clone(), vehicle_ids[2].clone()])
        .entity_map(&reuse)
        .send()
        .await?;
    expect_that!(two.items(), unordered_elements_are![has_id(&vehicle_ids[1]), has_id(&vehicle_ids[2])]);
    Ok(())
}
