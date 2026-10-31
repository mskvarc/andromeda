//! A partial update replaces the value a merge descends into.

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
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property, Value, ValuePatch},
};

/// Pins: Partial Attribute Update of an address value with only street replaces the whole value, while Merge Entity
/// setting street and a null country updates street, removes country and keeps city
/// Clause: 5.5.8, 5.5.12
#[gtest]
#[tokio::test]
async fn n_att_pat_9017_a_partial_update_replaces_the_value_a_merge_descends_into() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_value_object_partial_update_id = namespace.urn("Building", "value-object-partial-update")?;
    let building_type = namespace.entity_type("Building")?;
    let building_value_object_merge_id = namespace.urn("Building", "value-object-merge")?;
    let building_value_object_partial_update = Entity::builder()
        .id(building_value_object_partial_update_id.clone())
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
    broker.entities().create(&building_value_object_partial_update).send().await?.done()?;
    let patch = Attribute::from(Property::builder(object([("street", Value::from("Pariser Platz"))])).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_value_object_partial_update_id, &"address".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_value_object_partial_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "address",
            any_attribute()
                .value_member(["street"], "Pariser Platz")
                .without_value_member(["city"])
                .without_value_member(["country"])
        )
    );
    let building_value_object_merge = Entity::builder()
        .id(building_value_object_merge_id.clone())
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
    broker.entities().create(&building_value_object_merge).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_value_object_merge_id.clone(), building_type.clone())
        .patch(
            "address",
            Attribute::from(Property::merge_builder(ValuePatch::new().set("street", "Pariser Platz").delete("country")).build()?),
        )
        .build()?;
    let answer = broker
        .with_responses()
        .entities()
        .merge(&building_value_object_merge_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_value_object_merge_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
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
