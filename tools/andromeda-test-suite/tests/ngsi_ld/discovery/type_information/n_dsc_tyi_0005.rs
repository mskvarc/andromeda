//! A batch upsert replacing an entity drops what it no longer states.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    type_information::{attribute_names, families_of},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{AttributeKind, Entity, ErrorType},
};

/// Pins: a batch upsert in replace mode that gives an entity another type and other attributes takes away the types
/// and attributes it no longer states: a type and an attribute it alone carried are `ResourceNotFound`, the new
/// ones are counted, and a type another entity still holds keeps that entity
/// Clause: 5.7.7.4, 5.7.10.4, 5.6.8.4, 4.16, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0005_a_batch_upsert_replacing_an_entity_drops_what_it_no_longer_states() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let building_type = namespace.entity_type("Building")?;
    let store_type = namespace.entity_type("Store")?;
    let depot_type = namespace.entity_type("Depot")?;
    let height = namespace.attribute("height")?;
    let floors = namespace.attribute("floors")?;
    let owner = namespace.attribute("owner")?;
    let entities = [
        Entity::builder()
            .id(building_x_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .build()?,
        Entity::builder()
            .id(building_y_id.clone())
            .entity_type(vec![building_type.clone(), store_type.clone()])
            .property(namespace.attribute("height")?, 2)
            .property(namespace.attribute("owner")?, 3)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let replacement = [Entity::builder()
        .id(building_y_id)
        .entity_type(depot_type.clone())
        .property(namespace.attribute("floors")?, 4)
        .build()?];
    let answer = broker
        .with_responses()
        .entities()
        .batch_upsert(&replacement)
        .mode(UpsertMode::Replace)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .entity_types()
        .retrieve(&store_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let depot = broker.with_accept(Accept::any()).entity_types().retrieve(&depot_type).send().await?;
    expect_that!(depot.entity_count, eq(1));
    expect_that!(depot.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&depot), elements_are![eq(&floors)]);
    expect_that!(families_of(&depot, &floors), some(elements_are![eq(&AttributeKind::Property)]));
    let building = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(building.entity_count, eq(1));
    expect_that!(building.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&building), elements_are![eq(&height)]);
    expect_that!(families_of(&building, &height), some(elements_are![eq(&AttributeKind::Property)]));
    let answer_3 = broker.with_responses().with_accept(Accept::any()).attributes().retrieve(&owner).send().await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
