//! A batch merge admits the null at every depth.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, Entity, EntityFragment, NgsiLdMediaType, Property, Value, ValuePatch},
};

/// Pins: Batch Entity Merge with a Property value of urn:ngsi-ld:null and a key of another Property value set to it
/// answers 204, deletes the first Property and removes only that key of the second
/// Clause: 5.6.20.4, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9006_a_batch_merge_admits_the_null_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_merge_null_id = namespace.urn("Building", "batch-merge-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_merge_null = Entity::builder()
        .id(building_batch_merge_null_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .property(
            "address",
            object([
                ("street", Value::from("Strasse des 17. Juni")),
                ("city", Value::from("Berlin")),
                ("country", Value::from("Germany")),
            ]),
        )
        .build()?;
    broker.entities().create(&building_batch_merge_null).send().await?.done()?;
    let building_batch_merge_null_fragment = EntityFragment::for_entity(building_batch_merge_null_id.clone(), building_type.clone())
        .delete_kind("temperature", AttributeKind::Property)
        .patch(
            "address",
            Attribute::from(Property::merge_builder(ValuePatch::new().set("street", "Pariser Platz").delete("country")).build()?),
        )
        .build()?;
    let fragments = [building_batch_merge_null_fragment];
    let answer = broker.with_responses().entities().batch_merge(&fragments).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_merge_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("temperature"));
    expect_that!(
        entity,
        has_attribute(
            "address",
            any_attribute()
                .value_member(["street"], "Pariser Platz")
                .value_member(["city"], "Berlin")
                .without_value_member(["country"])
        )
    );
    Ok(())
}
