//! Following prev from the second page returns the first page.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with limit 2 over six members answers a first page whose next pointer
/// leads to a full second page, and that page's prev pointer leads back to the members of the first page
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0020_following_prev_from_the_second_page_returns_the_first_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let evolutions = (0..=5)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("Sensor", format!("{n:04}"))?)
                .entity_type(sensor_type.clone())
                .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&sensor_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&sensor_0001_id)));
    Ok(())
}
