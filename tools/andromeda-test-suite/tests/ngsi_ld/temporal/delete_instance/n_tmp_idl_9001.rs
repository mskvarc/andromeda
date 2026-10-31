//! An instance id of another entity is resource not found.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute Instance naming the `instanceId` of one Entity's speed under another Entity answers 404
/// `ResourceNotFound` and leaves that Entity's instance, and under the owning Entity answers 204
/// Clause: 5.6.15, 4.5.7, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_idl_9001_an_instance_id_of_another_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_uuid_privacy_owner_id = namespace.urn("Vehicle", "uuid-privacy-owner")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_uuid_privacy_other_id = namespace.urn("Vehicle", "uuid-privacy-other")?;
    let vehicle_uuid_privacy_owner_evolution = TemporalEntity::builder()
        .id(vehicle_uuid_privacy_owner_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_uuid_privacy_owner_evolution)
        .send()
        .await?
        .created()?;
    let vehicle_uuid_privacy_other_evolution = TemporalEntity::builder()
        .id(vehicle_uuid_privacy_other_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_uuid_privacy_other_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_uuid_privacy_owner_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance = instance_id_at(evolution, &"speed".parse()?, 0)?;
    let answer = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_uuid_privacy_other_id, &"speed".parse::<AttributeName>()?, &instance)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_uuid_privacy_other_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(1))));
    broker
        .temporal()
        .delete_instance(&vehicle_uuid_privacy_owner_id, &"speed".parse::<AttributeName>()?, &instance)
        .send()
        .await?;
    Ok(())
}
