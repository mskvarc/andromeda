//! With a configured api root prefix every pointer of a query starts with its path.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    link_header::link_target,
    namespace::Namespace,
    outcome::{Done, Upserted},
    paged_history::series,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, RelationType},
    model::{Entity, Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: On a broker whose configured apiRoot carries the path prefix /broker, Query Entities and Query Temporal
/// Evolution of Entities with limit 2 over three Entities each answer a next pointer starting with
/// /broker/ngsi-ld/v1/entities? and /broker/ngsi-ld/v1/temporal/entities?, and Retrieve Temporal Evolution of an
/// Entity holding 101 instances answers 206 with a next pointer starting with /broker/ngsi-ld/v1/temporal/entities/
/// Clause: 5.5.9, 6.2, 6.3.10
/// Recorded: Andromeda extension, not v1.9.1: `server.public_url` is the apiRoot, and a pointer starts with its path
/// (clause 6.2 allows a prefix string), so it resolves through the proxy that strips the prefix, as a Location does
/// Recorded: Andromeda configuration, not v1.9.1: the prefixed broker answers at most 100 instances per attribute
/// without lastN (clause 6.3.10 leaves it to the implementation)
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9006_with_a_configured_api_root_prefix_every_pointer_of_a_query_starts_with_its_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?;
    let item_type = namespace.entity_type("Item")?;
    let root = "/broker/ngsi-ld/v1";

    let items = (0..3)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Item", n)?)
                .entity_type(item_type.clone())
                .property("name", format!("i{n}"))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker_prefixed.entities().batch_create(&items).send().await?.done()?;
    let queried = broker_prefixed
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&item_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(
        link_target(queried.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/entities?")))
    );

    let sensor_type = namespace.entity_type("Sensor")?;
    for n in 0..3 {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Sensor", n)?)
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(n).observed_at("2020-01-01T00:00:00Z".parse()?))
            .build()?;
        broker_prefixed.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let temporal_queried = broker_prefixed
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(
        link_target(temporal_queried.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/temporal/entities?")))
    );

    let speed = (0..101_i32).map(|index| (index, index.unsigned_abs())).collect::<Vec<_>>();
    let history = series(&namespace, "prefixed", &speed, &[])?;
    let history_id = history.id.clone();
    broker_prefixed.temporal().create_or_update(&history).send().await?.created()?;
    let retrieved = broker_prefixed
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&history_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        link_target(retrieved.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/temporal/entities/")))
    );
    Ok(())
}
