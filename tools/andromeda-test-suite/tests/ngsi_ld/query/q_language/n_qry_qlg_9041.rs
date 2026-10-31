//! Every admitted fraction spelling of a bare operand names the same typed value.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Time, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a typed `readingTime` 12:00:00Z and a typed `readingInstant`
/// 2024-01-01T00:00:00Z to bare operands with no fraction, a short fraction and six fraction digits selects the
/// Entity for all five
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9041_every_admitted_fraction_spelling_of_a_bare_operand_names_the_same_typed_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_admitted_id = namespace.urn("Observation", "admitted")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_admitted = Entity::builder()
        .id(observation_admitted_id.clone())
        .entity_type(observation_type)
        .property("readingTime", Value::from("12:00:00Z".parse::<Time>()?))
        .property("readingInstant", Value::from("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_admitted).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_admitted_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_admitted_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00.0Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_admitted_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00.000000Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_admitted_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_admitted_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00.000000Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    Ok(())
}
