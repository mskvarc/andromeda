//! Join with key values renders linked entities simplified.

use andromeda_test_suite::{
    broker,
    core_context::core_context,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RepresentationSpelling},
    },
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, Value, VocabTerm},
};

/// Pins: Retrieve Entity with `options=keyValues` and a join renders the Linked Entities simplified: inline
/// replaces the Relationship by the simplified City, flat keeps the target URI and appends the simplified City, as
/// JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23.2, 4.5.23.3, 4.5.4
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0003_join_with_key_values_renders_linked_entities_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let city_type = namespace.entity_type("City")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_type = namespace.entity_type("Building")?;
    let building_typed_id = namespace.urn("Building", "typed")?;
    let building_untyped_id = namespace.urn("Building", "untyped")?;
    let entities = [
        Entity::builder().id(city_paris_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_typed_id.clone())
            .entity_type(building_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(city_paris_id.clone()).object_type(VocabTerm::from(&city_type)),
            )
            .build()?,
        Entity::builder()
            .id(building_untyped_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let linked_city = object([("id", Value::from(city_paris_id.to_string())), ("type", Value::from(city_type.as_str()))]);
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_typed_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(simplified_member_count(entity), eq(3));
    expect_that!(entity.id, eq(&building_typed_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_value("locatedAt", linked_city.clone()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_simplified(&building_typed_id)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(2)));
    expect_that!(entities_2.first().map(simplified_member_count), some(eq(3)));
    expect_that!(entities_2.first().map(|building| &building.id), some(eq(&building_typed_id)));
    expect_that!(
        entities_2.first().map(|building| &building.entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(entities_2.first(), some(has_value("locatedAt", city_paris_id.to_string())));
    expect_that!(entities_2.get(1).map(simplified_member_count), some(eq(2)));
    expect_that!(entities_2.get(1).map(|city| &city.id), some(eq(&city_paris_id)));
    expect_that!(entities_2.get(1).map(|city| &city.entity_type), some(eq(&OneOrMany::one(city_type.clone()))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_simplified(&building_untyped_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&building_untyped_id));
    expect_that!(entity_3.context, some(eq(&core_context()?)));
    expect_that!(entity_3, has_value("locatedAt", linked_city));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened_simplified(&building_untyped_id)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entities_4 = retrieved_4.body().as_slice();
    expect_that!(entities_4, len(eq(2)));
    expect_that!(entities_4.first().map(|building| &building.id), some(eq(&building_untyped_id)));
    expect_that!(entities_4.first(), some(has_value("locatedAt", city_paris_id.to_string())));
    expect_that!(entities_4.get(1).map(|city| &city.id), some(eq(&city_paris_id)));
    expect_that!(entities_4.get(1).map(|city| &city.entity_type), some(eq(&OneOrMany::one(city_type.clone()))));
    Ok(())
}
