//! A sub attribute date time is compared by equality and ordering.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a sub-attribute `DateTime` answers the entity both for the equal instant
/// and for an earlier lower bound
/// Clause: 4.9, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0021_a_sub_attribute_date_time_is_compared_by_equality_and_ordering() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property(
            "temperature",
            Property::builder(20).property("measuredAt", Value::from("2024-01-01T00:00:00Z".parse::<DateTime>()?)),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("measuredAt").eq("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("measuredAt").gt("2023-12-31T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
