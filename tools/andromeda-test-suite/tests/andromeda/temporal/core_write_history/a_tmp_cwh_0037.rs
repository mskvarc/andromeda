//! An update and a deletion behind a respond async create keep every instance.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchCreate},
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: on the broker defaults, Update Attributes setting v 2 and then Delete Attribute of v, both sent straight
/// after a Batch Entity Create answered 201 with Preference-Applied: respond-async of v 1, leave Retrieve Temporal
/// Evolution answering v 1 and v 2 once each, whether the queued history is applied yet or not
/// Clause: 4.3.1, 4.5.8, 5.6.3, 5.6.5, 5.7.3
/// Recorded: Prefer: respond-async on batch operations (IETF RFC 7240 section 4.1) queues the history the create
/// adds; the writes of the same Entity that follow are recorded behind it, and the instance the update recorded
/// survives the deletion that removes the attribute before the queue is applied
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0037_an_update_and_a_deletion_behind_a_respond_async_create_keep_every_instance() -> Result<()> {
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
    broker
        .entities()
        .delete_attribute(&sensor_id, &"v".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .temporal_q(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("v", all!(len(eq(2)), nth(0, property().value(1)), nth(1, property().value(2))))
    );
    instance_id_at(evolution, &"v".parse()?, 1)?;
    Ok(())
}
