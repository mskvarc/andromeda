//! A deferred core created entity gains no temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    eventually::retried,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::q,
};

/// Pins: on the deferred broker, Create Entity answers 201 and q=v==7 selects the Entity at once, while it has no
/// Temporal Evolution after a later Temporal API write has landed
/// Clause: 4.3.1, 5.6.1, 5.7.2, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded and `append_mode` deferred, which lands the Temporal API's history after the write answers
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0031_a_deferred_core_created_entity_gains_no_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(7).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?;
    broker_deferred.entities().create(&sensor_2).send().await?.done()?;
    let page = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").eq(7))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_2_id)));
    let marker = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("marker", Property::builder(1).observed_at("2026-07-01T00:59:00Z".parse()?))
        .build()?
        .attributes;
    broker_deferred.temporal().add_attributes(&sensor_id, &marker).send().await?;
    // The drain lands queued history in order, so once the marker written after the Core writes has landed, anything
    // those writes had queued would have landed too.
    retried(async || {
        let retrieved = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&sensor_id)
            .send()
            .await?;
        let evolution = retrieved.body();
        verify_that!(
            evolution,
            has_evolution("marker", contains(any_attribute().value(1).with_member(Member::InstanceId)))
        )?;
        Ok(())
    })
    .await?;
    let answer = broker_deferred
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_2_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
