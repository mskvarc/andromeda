//! A list property of strings keeps its order.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, ListProperty, NgsiLdMediaType, Value},
};

/// Pins: a `ListProperty` of four strings keeps every value in the order written through create and retrieve
/// Clause: 4.5.21, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0017_a_list_property_of_strings_keeps_its_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .list_property(
            "labels",
            ListProperty::builder(vec![Value::from("alpha"), Value::from("beta"), Value::from("gamma"), Value::from("delta")]),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "labels"), some(eq(2)));
    expect_that!(
        entity,
        has_attribute(
            "labels",
            attribute_of(AttributeKind::ListProperty)
                .list_length(4)
                .list_value(0, "alpha")
                .list_value(1, "beta")
                .list_value(2, "gamma")
                .list_value(3, "delta")
        )
    );
    Ok(())
}
