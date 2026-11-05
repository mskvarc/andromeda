//! Deleting the temporal evolution straight after a respond async update leaves none.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpdate},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Temporal Evolution sent straight after a Batch Entity Update answered 204 with Preference-Applied:
/// respond-async answers 204, and Retrieve Temporal Evolution sent straight after it, with no retry, answers 404
/// `ResourceNotFound`: the history the update queued is deleted with the Temporal Evolution, never left to land after it
/// Clause: 5.6.9, 5.6.16, 5.7.3
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write; clause 5.6.16 deletes the Temporal Evolution whole, so nothing the
/// update queued before the deletion may remain of it
#[gtest]
#[tokio::test]
async fn a_bat_upd_0016_deleting_the_temporal_evolution_straight_after_a_respond_async_update_leaves_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("level", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(sensor_id.clone(), sensor_type.clone())
        .property("level", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?];
    let request = respond_async(broker.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    broker.temporal().delete(&sensor_id).send().await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
