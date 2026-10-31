//! A respond async upsert creating an entity gains no temporal evolution.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::problem,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: on the not-recorded broker, a Batch Entity Upsert creating an Entity with v 7 answered 201 with
/// Preference-Applied: respond-async is selected by q=v==7, while Retrieve Temporal Evolution sent straight after
/// it, with no retry, answers 404 `ResourceNotFound`
/// Clause: 4.3.1, 5.6.8, 5.7.2, 5.7.3
/// Recorded: Andromeda configuration and extension, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core
/// API write updates the Temporal Evolution ("should consider updating"); this broker runs with
/// `core_write_history` not-recorded, and Prefer: respond-async on batch operations (IETF RFC 7240 section 4.1)
/// queues the history a recorded write adds, which every temporal read answers at once
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0034_a_respond_async_upsert_creating_an_entity_gains_no_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let entities = [Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(7).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?];
    let request = respond_async(broker_not_recorded.unchecked(&BatchUpsert::builder().entities(&entities).build())?);
    let answer = broker_not_recorded.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    // Asked at once, with no retry.
    let absent = broker_not_recorded
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").eq(7))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
