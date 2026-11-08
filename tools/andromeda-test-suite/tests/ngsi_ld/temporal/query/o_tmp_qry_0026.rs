//! Following prev without local returns the first page.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with limit 2 over six members answers a first page whose next pointer
/// leads to a full second page, and that page's prev pointer leads back to the members of the first page
/// Clause: 5.5.9.1, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0026_following_prev_without_local_returns_the_first_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0003_id = namespace.urn("Sensor", "0003")?;
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
        tenant.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&sensor_0001_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&sensor_0003_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&sensor_0001_id)));
    Ok(())
}
