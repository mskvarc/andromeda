//! Attrs answers only the listed attributes.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    core_context::core_entry,
    entity_matcher::{has_attribute, lacks_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, lacks_value, simplified_member_count},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, Entity, NgsiLdMediaType, OneOrMany, Property},
};

/// Pins: Retrieve Entity with attrs answers the entity with only the listed attributes, whether they are named by
/// full IRI or by term, and with `options=keyValues`
/// Clause: 5.7.1, 5.5.7, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0002_attrs_answers_only_the_listed_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 42)
            .property("color", "red")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // attributes named by their full IRI under the default vocabulary
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .attrs(vec![
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel".parse()?,
            "https://uri.etsi.org/ngsi-ld/default-context/subCategory".parse()?,
        ])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(5));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, lacks_attribute("name"));
    expect_that!(entity, lacks_attribute("almostFull"));
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            property().value(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?)
        )
    );
    expect_that!(entity, has_attribute("subCategory", property().value("tourism")));
    // the same attributes named by term
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["airQualityLevel".parse()?, "subCategory".parse()?])
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(4));
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity_2, lacks_attribute("name"));
    expect_that!(entity_2, lacks_attribute("almostFull"));
    expect_that!(entity_2, has_attribute("airQualityLevel", any_attribute().value(4)));
    expect_that!(entity_2, has_attribute("subCategory", any_attribute().value("tourism")));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .attrs(vec!["speed".parse()?])
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(simplified_member_count(entity_3), eq(3));
    expect_that!(entity_3.id, eq(&vehicle_id));
    expect_that!(entity_3.entity_type, eq(&OneOrMany::one(vehicle_type.clone())));
    expect_that!(entity_3, has_value("speed", 42));
    expect_that!(entity_3, lacks_value("color"));
    Ok(())
}
