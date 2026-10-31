//! A respond async create updated at once records each instance once.

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

/// Pins: on the broker defaults, a Batch Entity Create answered 201 with Preference-Applied: respond-async of v 1,
/// followed at once by Update Attributes setting v 2, leaves Retrieve Temporal Evolution answering v 1 and v 2 once
/// each, whether the queued history is applied yet or not
/// Clause: 4.3.1, 4.5.7, 5.6.3, 5.7.3
/// Recorded: Prefer: respond-async on batch operations (IETF RFC 7240 section 4.1) queues the history the create
/// adds; the update that rewrites the attribute before that history is applied records its own instance behind it,
/// and the queued instance is recorded once whichever of them is applied first
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0036_a_respond_async_create_updated_at_once_records_each_instance_once() -> Result<()> {
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
    expect_that!(
        retrieved.body(),
        has_evolution("v", all!(len(eq(2)), nth(0, property().value(1)), nth(1, property().value(2))))
    );
    Ok(())
}
