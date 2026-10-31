//! A quoted operand is a string whatever its separator and matches no typed time value.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Time, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a typed `readingTime` 12:00:00.500000Z and a typed `readingInstant`
/// 2024-01-01T00:00:00.250000Z to a quoted operand, with a comma or a decimal point, selects nothing, and the bare
/// operand with fewer fraction digits selects the Entity
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9039_a_quoted_operand_is_a_string_whatever_its_separator_and_matches_no_typed_time_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_comma_id = namespace.urn("Observation", "comma")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_comma = Entity::builder()
        .id(observation_comma_id.clone())
        .entity_type(observation_type)
        .property("readingTime", Value::from("12:00:00.500000Z".parse::<Time>()?))
        .property("readingInstant", Value::from("2024-01-01T00:00:00.250000Z".parse::<DateTime>()?))
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_comma).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00,5Z"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00.5Z"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00,25Z"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00.25Z"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingTime").eq("12:00:00.5Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_comma_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00.25Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    Ok(())
}
