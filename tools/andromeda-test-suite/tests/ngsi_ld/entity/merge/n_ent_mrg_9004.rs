//! A merge admits the null under a key of a property value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, Entity, EntityFragment, NgsiLdMediaType, Property, Value, ValuePatch},
};

/// Pins: Merge Entity setting street and setting country to urn:ngsi-ld:null in an address value answers 204,
/// updates street, removes country and keeps city
/// Clause: 5.5.12, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9004_a_merge_admits_the_null_under_a_key_of_a_property_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_nested_null_id = namespace.urn("Building", "merge-nested-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merge_nested_null = Entity::builder()
        .id(building_merge_nested_null_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([
                ("street", Value::from("Strasse des 17. Juni")),
                ("city", Value::from("Berlin")),
                ("country", Value::from("Germany")),
            ]),
        )
        .build()?;
    broker.entities().create(&building_merge_nested_null).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_merge_nested_null_id.clone(), building_type.clone())
        .patch(
            "address",
            Attribute::from(Property::merge_builder(ValuePatch::new().set("street", "Pariser Platz").delete("country")).build()?),
        )
        .build()?;
    let answer = broker.with_responses().entities().merge(&building_merge_nested_null_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_nested_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
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
