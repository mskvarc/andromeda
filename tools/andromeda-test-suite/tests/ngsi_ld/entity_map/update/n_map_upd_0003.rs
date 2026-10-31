//! A json ld fragment with its context sets the expiry.

use andromeda_test_suite::{broker, core_context::core_context_list, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: Update `EntityMap` with an application/ld+json fragment naming its @context answers 204, and the
/// `EntityMap` then reads back with the written `expiresAt`
/// Clause: 5.14.2.3, 5.14.2.4, 6.3.5
#[gtest]
#[tokio::test]
async fn n_map_upd_0003_a_json_ld_fragment_with_its_context_sets_the_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let plus_4h: DateTime = (Utc::now() + TimeDelta::hours(4)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    let body_context = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    body_context.entity_maps().update(map_id, &plus_4h).send().await?;
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map.expires_at, eq(&plus_4h));
    Ok(())
}
