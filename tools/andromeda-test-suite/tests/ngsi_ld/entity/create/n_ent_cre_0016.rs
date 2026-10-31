//! A list property reads back with its value list.

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

/// Pins: a `ListProperty` reads back in the normalized representation with its type and its `valueList`, in order
/// Clause: 4.5.21.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0016_a_list_property_reads_back_with_its_value_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .list_property(
            "tags",
            ListProperty::builder(vec![Value::from("residential"), Value::from("commercial"), Value::from("mixed-use")]),
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
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    expect_that!(
        entity,
        has_attribute(
            "tags",
            attribute_of(AttributeKind::ListProperty)
                .list_length(3)
                .list_value(0, "residential")
                .list_value(1, "commercial")
                .list_value(2, "mixed-use")
        )
    );
    Ok(())
}
