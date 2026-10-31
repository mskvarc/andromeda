//! A query last n takes the most recent observed instances of each entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities over two Entities whose speed each holds observed instances and
/// instances with no `observedAt` answers `lastN` 1 after 2000-01-01, by GET and by POST, with each Entity's most
/// recent observed instance: 20 and 200
/// Clause: 5.7.4.4, 4.11, Table 5.2.21-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9066_a_query_last_n_takes_the_most_recent_observed_instances_of_each_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_evolution = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", 1000)
        .build()?;
    broker.temporal().create_or_update(&device_evolution).send().await?.created()?;
    let device_2_evolution = TemporalEntity::builder()
        .id(device_2_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2000)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T03:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_2_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).last_n(1))
        .entity_type(&device_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&device_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20))))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&device_2_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(200))))
        ))
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).last_n(1))
        .method(QueryMethod::Post)
        .entity_type(&device_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&device_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20))))
        ))
    );
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&device_2_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(200))))
        ))
    );
    Ok(())
}
