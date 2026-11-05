//! Deleting the temporal evolution straight after a respond async create removes its queued history.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchCreate},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Delete Temporal Evolution sent straight after a Batch Entity Create answered 201 with Preference-Applied:
/// respond-async of v 1 answers 204, the Entity keeps its current state, and after Update Attributes setting v 2,
/// Retrieve Temporal Evolution answers v 2 alone, whether the create's history was still queued at the deletion or not
/// Clause: 5.6.7, 5.6.16, 5.6.3, 5.7.3
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1) queues the history the create adds; reads count that history as recorded, so clause 5.6.16 deletes the
/// Temporal Evolution it brings into being, and none of it lands after the deletion
#[gtest]
#[tokio::test]
async fn a_bat_cre_9012_deleting_the_temporal_evolution_straight_after_a_respond_async_create_removes_its_queued_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let entities = [Entity::builder()
        .id(sensor_id.clone())
        .entity_type(namespace.entity_type("Sensor")?)
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?];
    let request = respond_async(broker.unchecked(&BatchCreate::builder().entities(&entities).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::CREATED));
    broker.temporal().delete(&sensor_id).send().await?;
    broker.entities().retrieve(&sensor_id).send().await?;
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(2).observed_at("2026-07-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .temporal_q(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("v", all!(len(eq(1)), nth(0, property().value(2)))));
    Ok(())
}
