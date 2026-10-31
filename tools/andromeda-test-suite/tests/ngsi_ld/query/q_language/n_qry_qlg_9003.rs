//! A q on unit code selects by its string.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q on `temperature.unitCode` selects the reading whose `unitCode` is the quoted string
/// and not the other
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9003_a_q_on_unit_code_selects_by_its_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unit_celsius_id = namespace.urn("Building", "unit-celsius")?;
    let building_type = namespace.entity_type("Building")?;
    let building_unit_fahrenheit_id = namespace.urn("Building", "unit-fahrenheit")?;
    let building_unit_celsius = Entity::builder()
        .id(building_unit_celsius_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .observed_at("2024-06-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.5".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_unit_celsius).send().await?.done()?;
    let building_unit_fahrenheit = Entity::builder()
        .id(building_unit_fahrenheit_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Fah)
                .observed_at("2024-06-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.5".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_unit_fahrenheit).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_unit_celsius_id.clone()])
        .q(q::attr("temperature").sub_attr("unitCode").eq("CEL"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_unit_fahrenheit_id.clone()])
        .q(q::attr("temperature").sub_attr("unitCode").eq("CEL"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_unit_fahrenheit_id.clone()])
        .q(q::attr("temperature").sub_attr("unitCode").eq("FAH"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_unit_celsius_id.clone()])
        .q(q::attr("temperature").sub_attr("unitCode").eq("FAH"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
