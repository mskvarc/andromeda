//! A string only resembling a time value compares as text.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q selects the Entity for `readingNote` equal to its colon-bearing text and unequal to
/// 09:30:00Z, nothing for the untyped offset-bearing `readingLabel` compared to a `DateTime` by ==, bare or quoted,
/// and the Entity for `readingLabel` compared by > and < to a quoted operand in the order of their text
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9045_a_string_only_resembling_a_time_value_compares_as_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_resemblance_id = namespace.urn("Observation", "resemblance")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_resemblance = Entity::builder()
        .id(observation_resemblance_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00Z")
        .property("readingInstant", "2024-06-15T12:00:00Z")
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_resemblance).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingNote").eq("12:00:00 in the morning"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingNote").ne("09:30:00Z"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingLabel").eq("2024-01-01T00:00:00Z"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingLabel").eq("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingLabel").gt("2023-01-01T00:00:00Z"))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_resemblance_id.clone()])
        .q(q::attr("readingLabel").lt("2025-01-01T00:00:00Z"))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    Ok(())
}
