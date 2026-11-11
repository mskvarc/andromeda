//! Type alternatives serve a map through either binding.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder, type_selection},
};

/// Pins: Query Entities through POST with the `EntitySelector`s of types A and B under an `EntityMap` created through
/// GET with type A|B answers both mapped Entities, and with the `EntitySelector` of type A alone answers 400
/// `InvalidRequest`, and Query Entities through GET with type B|A under an `EntityMap` created through POST with the
/// `EntitySelector`s of types A and B answers both mapped Entities
/// Clause: 5.5.14, 5.7.2.3, 5.2.33, 4.17
/// Tenant: private (the broker forwards a query of several selectors to registrations of other types too, so in the
/// shared tenant it reaches other files' sources)
#[gtest]
#[tokio::test]
async fn n_map_qen_9013_type_alternatives_serve_a_map_through_either_binding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 50)
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("floors", 3)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?),
        EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?),
    ];
    let created_through_get = json
        .with_heads()
        .entity_maps()
        .create()
        .entity_type(type_selection::entity_type(&vehicle_type).or(&building_type))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let get_map_request = EntityMapRequest::reuse(created_through_get.entity_map().valid().or_fail()?.clone());
    let posted_page = json
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors.clone())
        .entity_map(&get_map_request)
        .send()
        .await?;
    expect_that!(posted_page.items(), unordered_elements_are![has_id(&vehicle_id), has_id(&building_id)]);
    let answer = json
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .entity_map(&get_map_request)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let created_through_post = json
        .with_heads()
        .entity_maps()
        .create()
        .selectors(selectors)
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let post_map_request = EntityMapRequest::reuse(created_through_post.entity_map().valid().or_fail()?.clone());
    let got_page = json
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&building_type).or(&vehicle_type))
        .entity_map(&post_map_request)
        .send()
        .await?;
    expect_that!(got_page.items(), unordered_elements_are![has_id(&vehicle_id), has_id(&building_id)]);
    broker.entity_maps().delete(&created_through_get.body().id).send().await?;
    broker.entity_maps().delete(&created_through_post.body().id).send().await?;
    Ok(())
}
