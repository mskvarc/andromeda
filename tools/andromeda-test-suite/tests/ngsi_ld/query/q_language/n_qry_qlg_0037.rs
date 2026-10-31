//! A time value is compared as a time of day whatever its fraction.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Time, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a Time attribute matches the time of day whatever fractional width the
/// value and the operand are written with
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0037_a_time_value_is_compared_as_a_time_of_day_whatever_its_fraction() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shift_id = namespace.urn("Shift", 1)?;
    let shift_type = namespace.entity_type("Shift")?;
    let shift_2_id = namespace.urn("Shift", 2)?;
    let shift = Entity::builder()
        .id(shift_id.clone())
        .entity_type(shift_type.clone())
        .property("opens", Value::from("08:00:00Z".parse::<Time>()?))
        .build()?;
    broker.entities().create(&shift).send().await?.done()?;
    let shift_2 = Entity::builder()
        .id(shift_2_id.clone())
        .entity_type(shift_type.clone())
        .property("opens", Value::from("08:00:00.500Z".parse::<Time>()?))
        .build()?;
    broker.entities().create(&shift_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("opens").eq("08:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&shift_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("opens").eq("08:00:00.000Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&shift_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("opens").eq("08:00:00.5Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&shift_2_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("opens").gt("08:00:00Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&shift_2_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr("opens").le("08:00:00.000Z".parse::<Time>()?))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&shift_id)));
    Ok(())
}
