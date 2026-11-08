//! A named tenant leaves core writes out of history too.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: in a named tenant, Update Attributes of a seeded Entity and Create Entity of another answer 204 and 201
/// and current state takes them, while the seeded Entity holds only its seeded v instances and the created one has
/// no Temporal Evolution
/// Clause: 4.3.1, 4.14, 5.6.1, 5.6.2, 5.7.3
/// Tenant: private (the subject is the setting holding in a named tenant)
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0029_a_named_tenant_leaves_core_writes_out_of_history_too() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let broker_not_recorded_tenant = broker::private_tenant(&broker_not_recorded, &namespace).await?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded_tenant
        .temporal()
        .create_or_update(&sensor_evolution)
        .send()
        .await?
        .created()?;
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(5).observed_at("2026-07-01T00:02:00Z".parse()?))
        .build()?;
    broker_not_recorded_tenant
        .entities()
        .update_attributes(&sensor_id, &fragment)
        .send()
        .await?
        .done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(7).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?;
    broker_not_recorded_tenant.entities().create(&sensor_2).send().await?.done()?;
    let retrieved = broker_not_recorded_tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(5)));
    let retrieved_2 = broker_not_recorded_tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(
                    any_attribute()
                        .value(1)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(2)
                        .observed_at("2026-07-01T00:01:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    let answer = broker_not_recorded_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_2_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
