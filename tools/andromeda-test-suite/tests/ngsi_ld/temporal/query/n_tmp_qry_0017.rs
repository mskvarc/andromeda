//! A page reached backward holding the first members offers no prev.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with limit 2 over four members, followed by next and then by the
/// second page's prev pointer, answers the first two members with no prev pointer, there being no previous members
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0017_a_page_reached_backward_holding_the_first_members_offers_no_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0003_id = namespace.urn("Sensor", "0003")?;
    let sensor_0000_evolution = TemporalEntity::builder()
        .id(sensor_0000_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0000_evolution).send().await?.created()?;
    let sensor_0001_evolution = TemporalEntity::builder()
        .id(sensor_0001_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0001_evolution).send().await?.created()?;
    let sensor_0002_evolution = TemporalEntity::builder()
        .id(sensor_0002_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0002_evolution).send().await?.created()?;
    let sensor_0003_evolution = TemporalEntity::builder()
        .id(sensor_0003_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0003_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&sensor_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&sensor_0003_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.prev(), none());
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&sensor_0001_id)));
    Ok(())
}
