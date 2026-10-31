//! A map is retrieved from the broker alone beside a registration advertising the retrieve operation through its group.

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
    model::{Entity, NgsiLdMediaType},
    registration::EntitySource,
};

/// Pins: Retrieve `EntityMap` beside a registration advertising `federationOps`, which holds `retrieveEntityMap`,
/// answers 200 with the map the broker holds and sends nothing to the source, whose registration reports no forward
/// Clause: 5.14.1.4, 4.20
#[gtest]
#[tokio::test]
async fn n_map_ret_9119_a_map_is_retrieved_from_the_broker_alone_beside_a_registration_advertising_retrieve_entity_map_through_its_group() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
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
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(map_id).send().await?;
    expect_that!(&map.id, eq(map_id));
    expect_that!(
        map.entity_map.get(&vehicle_id).and_then(|sources| sources.first()),
        some(eq(&EntitySource::Local))
    );
    expect_unreached(&source, &SourceRequest::any_matching("/a/.*")).await?;
    expect_no_forward_reported(&broker, &registration_id).await?;
    Ok(())
}
