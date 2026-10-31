//! Key values renders a list property as the array of its values.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_length_at, has_value_at},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, ListProperty, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity with `options=keyValues` answers a `ListProperty` as the bare array of its values, in
/// order
/// Clause: 4.5.4, 4.5.21, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0021_key_values_renders_a_list_property_as_the_array_of_its_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .list_property("tags", ListProperty::builder(vec![Value::from(1), Value::from(2), Value::from(3)]))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_length_at("tags", [], 3));
    expect_that!(entity, has_value_at("tags", ["0"], 1));
    expect_that!(entity, has_value_at("tags", ["1"], 2));
    expect_that!(entity, has_value_at("tags", ["2"], 3));
    Ok(())
}
