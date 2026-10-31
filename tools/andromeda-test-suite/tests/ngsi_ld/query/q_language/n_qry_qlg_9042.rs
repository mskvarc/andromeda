//! A typed date time answers every operator and a quoted operand only unequal.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a typed `DateTime` Property value by ==, !=, >, >=, < and <= selects the
/// Entity exactly where the temporal order relation holds for the bare operand, and for the quoted operand, a
/// String, only by !=
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9042_a_typed_date_time_answers_every_operator_and_a_quoted_operand_only_unequal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_instant_operators_id = namespace.urn("Observation", "instant-operators")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_instant_operators = Entity::builder()
        .id(observation_instant_operators_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00Z")
        .property("readingInstant", Value::from("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_instant_operators).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:00.000000Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:00.000000Z"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:01Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(0)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").eq("2024-06-15T12:00:01Z"))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ne("2024-06-15T12:00:01Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ne("2024-06-15T12:00:01Z"))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(1)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ne("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(0)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ne("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(1)));
    let page_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").gt("2024-06-15T11:59:59Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_11.items(), len(eq(1)));
    let page_12 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").gt("2024-06-15T11:59:59Z"))
        .send()
        .await?;
    expect_that!(page_12.items(), len(eq(0)));
    let page_13 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").gt("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_13.items(), len(eq(0)));
    let page_14 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").gt("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_14.items(), len(eq(0)));
    let page_15 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ge("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_15.items(), len(eq(1)));
    let page_16 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ge("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_16.items(), len(eq(0)));
    let page_17 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ge("2024-06-15T12:00:01Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_17.items(), len(eq(0)));
    let page_18 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").ge("2024-06-15T12:00:01Z"))
        .send()
        .await?;
    expect_that!(page_18.items(), len(eq(0)));
    let page_19 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").lt("2024-06-15T12:00:01Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_19.items(), len(eq(1)));
    let page_20 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").lt("2024-06-15T12:00:01Z"))
        .send()
        .await?;
    expect_that!(page_20.items(), len(eq(0)));
    let page_21 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").lt("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_21.items(), len(eq(0)));
    let page_22 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").lt("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_22.items(), len(eq(0)));
    let page_23 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").le("2024-06-15T12:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_23.items(), len(eq(1)));
    let page_24 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").le("2024-06-15T12:00:00Z"))
        .send()
        .await?;
    expect_that!(page_24.items(), len(eq(0)));
    let page_25 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").le("2024-06-15T11:59:59Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_25.items(), len(eq(0)));
    let page_26 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_operators_id.clone()])
        .q(q::attr("readingInstant").le("2024-06-15T11:59:59Z"))
        .send()
        .await?;
    expect_that!(page_26.items(), len(eq(0)));
    Ok(())
}
