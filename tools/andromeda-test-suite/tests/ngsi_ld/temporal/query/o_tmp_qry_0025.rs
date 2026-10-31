//! Without local the walk goes by cursor over every entity.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities without local, limit 2 over seven entities, answers cursor pointers,
/// and following its next pointers answers every entity once in ascending identifier order
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0025_without_local_the_walk_goes_by_cursor_over_every_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0003_id = namespace.urn("Sensor", "0003")?;
    let sensor_0004_id = namespace.urn("Sensor", "0004")?;
    let sensor_0005_id = namespace.urn("Sensor", "0005")?;
    let sensor_0006_id = namespace.urn("Sensor", "0006")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let evolutions = (0..=6)
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
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
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
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_0004_id)));
    expect_that!(page_3.items(), nth(1, has_id(&sensor_0005_id)));
    let page_4 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_3.next_page().or_fail()?)
        .await?;
    expect_that!(page_4.next(), none());
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&sensor_0006_id)));
    Ok(())
}
