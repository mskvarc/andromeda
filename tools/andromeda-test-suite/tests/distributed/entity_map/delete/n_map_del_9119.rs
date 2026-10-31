//! A map is deleted on the broker alone beside a registration advertising the delete operation through its group.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    federation_ops_source::{expect_no_forward_reported, expect_unreached, register_federation_ops_source},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Delete `EntityMap` beside a registration advertising `federationOps`, which holds `deleteEntityMap`, answers
/// 204, leaves the map not found and sends nothing to the source, whose registration reports no forward
/// Clause: 5.14.3.4, 4.20
#[gtest]
#[tokio::test]
async fn n_map_del_9119_a_map_is_deleted_on_the_broker_alone_beside_a_registration_advertising_delete_entity_map_through_its_group() -> Result<()> {
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
    broker.entity_maps().delete(map_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    expect_unreached(&source, &SourceRequest::any_matching("/a/.*")).await?;
    expect_no_forward_reported(&broker, &registration_id).await?;
    Ok(())
}
