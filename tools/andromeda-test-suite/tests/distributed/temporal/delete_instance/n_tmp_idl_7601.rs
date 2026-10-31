//! An instance held only by the peer is resource not found.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, ErrorType, InstanceId, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute instance on broker A of an `instanceId` held only in broker B's evolution of speed, while
/// broker A holds speed under another `instanceId` and no registration names B, answers 404 `ResourceNotFound` and
/// leaves broker B's evolution unchanged
/// Clause: 5.6.15.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_tmp_idl_7601_an_instance_held_only_by_the_peer_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Mercedes").observed_at("2021-01-01T00:00:00Z".parse()?))
        .identified(
            "speed",
            format!("urn:ngsi-ld:Instance:{namespace}-1").parse::<InstanceId>()?,
            Property::builder(42).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .identified(
            "speed",
            format!("urn:ngsi-ld:Instance:{namespace}-2").parse::<InstanceId>()?,
            Property::builder(42).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let instance_id_2 = format!("urn:ngsi-ld:Instance:{namespace}-1").parse::<InstanceId>()?;
    let answer = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id_2)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker_b.with_accept(Accept::any()).temporal().retrieve(&vehicle_id).send().await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(42).instance_id(&instance_id_2))))
    );
    Ok(())
}
