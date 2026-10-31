//! A map is updated on the broker alone beside a registration advertising the update operation through its group.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    federation_ops_source::{expect_no_forward_reported, expect_unreached, register_federation_ops_source},
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: Update `EntityMap` beside a registration advertising `federationOps`, which holds `updateEntityMap`, answers
/// 204, reads back the new `expiresAt` and sends nothing to the source, whose registration reports no forward
/// Clause: 5.14.2.4, 4.20
#[gtest]
#[tokio::test]
async fn n_map_upd_9119_a_map_is_updated_on_the_broker_alone_beside_a_registration_advertising_update_entity_map_through_its_group() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
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
    let registration_id = register_federation_ops_source(&broker, &source, &namespace).await?;
    let expires_at: DateTime = "2999-01-01T00:00:00Z".parse()?;
    broker.entity_maps().update(map_id, &expires_at).send().await?;
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(map_id).send().await?;
    expect_that!(&map.expires_at, eq(&expires_at));
    expect_unreached(&source, &SourceRequest::any_matching("/a/.*")).await?;
    expect_no_forward_reported(&broker, &registration_id).await?;
    Ok(())
}
