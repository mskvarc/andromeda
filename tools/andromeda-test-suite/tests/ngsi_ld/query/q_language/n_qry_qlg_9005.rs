//! A q on a user defined sub property selects by its value.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: Query Entities with q temperature.accuracy>0.7 selects the reading whose accuracy is 0.9 and not the one
/// whose accuracy is 0.2
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9005_a_q_on_a_user_defined_sub_property_selects_by_its_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_accuracy_precise_id = namespace.urn("Building", "accuracy-precise")?;
    let building_type = namespace.entity_type("Building")?;
    let building_accuracy_coarse_id = namespace.urn("Building", "accuracy-coarse")?;
    let building_accuracy_precise = Entity::builder()
        .id(building_accuracy_precise_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .observed_at("2024-06-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_accuracy_precise).send().await?.done()?;
    let building_accuracy_coarse = Entity::builder()
        .id(building_accuracy_coarse_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .observed_at("2024-06-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.2".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_accuracy_coarse).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_accuracy_precise_id.clone()])
        .q(q::attr("temperature").sub_attr("accuracy").gt("0.7".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_accuracy_coarse_id.clone()])
        .q(q::attr("temperature").sub_attr("accuracy").gt("0.7".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
