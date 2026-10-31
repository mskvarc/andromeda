//! A q on observed at selects by its datetime.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q on `temperature.observedAt` selects the June reading and not the January one under
/// >, == and a bounded range of `dateTime` values
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9002_a_q_on_observed_at_selects_by_its_datetime() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_observed_hot_id = namespace.urn("Building", "observed-hot")?;
    let building_type = namespace.entity_type("Building")?;
    let building_observed_cold_id = namespace.urn("Building", "observed-cold")?;
    let building_observed_hot = Entity::builder()
        .id(building_observed_hot_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .observed_at("2024-06-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.5".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_observed_hot).send().await?.done()?;
    let building_observed_cold = Entity::builder()
        .id(building_observed_cold_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(20)
                .unit_code(UnitCode::Fah)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.entities().create(&building_observed_cold).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_hot_id.clone()])
        .q(q::attr("temperature").sub_attr("observedAt").gt("2024-03-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_cold_id.clone()])
        .q(q::attr("temperature").sub_attr("observedAt").gt("2024-03-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_hot_id.clone()])
        .q(q::attr("temperature").sub_attr("observedAt").eq("2024-06-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_cold_id.clone()])
        .q(q::attr("temperature").sub_attr("observedAt").eq("2024-06-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_hot_id.clone()])
        .q(q::attr("temperature")
            .sub_attr("observedAt")
            .gt("2024-03-01T00:00:00Z".parse::<DateTime>()?)
            .and(q::attr("temperature").sub_attr("observedAt").lt("2024-09-01T00:00:00Z".parse::<DateTime>()?)))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_observed_cold_id.clone()])
        .q(q::attr("temperature")
            .sub_attr("observedAt")
            .gt("2024-03-01T00:00:00Z".parse::<DateTime>()?)
            .and(q::attr("temperature").sub_attr("observedAt").lt("2024-09-01T00:00:00Z".parse::<DateTime>()?)))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    Ok(())
}
