//! Simplified renders operation space as its geometry.

use andromeda_test_suite::{broker, core_context::core_entry, namespace::Namespace, outcome::Done, simplified_matcher::simplified_member_count};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        AttributeName,
        Context,
        Entity,
        NgsiLdMediaType,
        OneOrMany,
        SimplifiedValue,
        geometry::{Geometry, LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: Retrieve Entity renders the `operationSpace` `GeoProperty` as its bare geometry in the simplified
/// representation, as JSON and as JSON-LD
/// Clause: 4.7, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0012_simplified_renders_operation_space_as_its_geometry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let operation_space = Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
        Position::try_new(100.12, 0.23)?,
        Position::try_new(101.12, 0.23)?,
        Position::try_new(101.12, 1.23)?,
        Position::try_new(100.12, 1.23)?,
        Position::try_new(100.12, 0.23)?,
    ])?])?);
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("operationSpace", operation_space.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let operation_space_name = "operationSpace".parse::<AttributeName>()?;
    let expected = SimplifiedValue::Geometry(Geometry::from(operation_space));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(simplified_member_count(entity), eq(3));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity.attributes.get(&operation_space_name), some(eq(&expected)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(simplified_member_count(entity_2), eq(4));
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(entity_2.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_2.attributes.get(&operation_space_name), some(eq(&expected)));
    Ok(())
}
