//! Every quantity bearing family stores a unit.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, geo_property, property},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{AttributeKind, Entity, ListProperty, Lossless, NgsiLdMediaType, Property, RawJson, Value},
};

/// Pins: Create Entity with a `unitCode` on a Property, a `GeoProperty` and a `ListProperty` answers 201 each, and
/// each reads back with `unitCode` MMT as a member of its own family
/// Clause: 4.5.2.2, 4.5.21.2, 4.7.3
#[gtest]
#[tokio::test]
async fn n_cmn_uni_9001_every_quantity_bearing_family_stores_a_unit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unit_property_id = namespace.urn("Building", "unit-Property")?;
    let building_type = namespace.entity_type("Building")?;
    let building_unit_geo_property_id = namespace.urn("Building", "unit-GeoProperty")?;
    let building_unit_list_property_id = namespace.urn("Building", "unit-ListProperty")?;
    let building_unit_property = Entity::builder()
        .id(building_unit_property_id.clone())
        .entity_type(building_type.clone())
        .property("measure", Property::builder(42).unit_code(UnitCode::Mmt))
        .build()?;
    broker.entities().create(&building_unit_property).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_unit_property_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("measure", property().unit_code(UnitCode::Mmt)));
    let building_unit_geo_property = Entity::builder()
        .id(building_unit_geo_property_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a GeoProperty with unitCode or valueType), so `measure` is written as text.
    let measure: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": "MMT"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_unit_geo_property, Lossless::Normalized).set_member("measure", &measure);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_unit_geo_property).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_unit_geo_property_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("measure", geo_property().unit_code(UnitCode::Mmt)));
    let building_unit_list_property = Entity::builder()
        .id(building_unit_list_property_id.clone())
        .entity_type(building_type.clone())
        .list_property(
            "measure",
            ListProperty::builder(vec![Value::from(300), Value::from(300), Value::from(120), Value::from(6)]).unit_code(UnitCode::Mmt),
        )
        .build()?;
    broker.entities().create(&building_unit_list_property).send().await?.done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_unit_list_property_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute("measure", attribute_of(AttributeKind::ListProperty).unit_code(UnitCode::Mmt))
    );
    Ok(())
}
