//! A deletion straight after a respond async create is recorded.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Done,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchCreate},
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: on the broker defaults, Delete Attribute sent straight after a Batch Entity Create answered 201 with
/// Preference-Applied: respond-async records its deletion of v, whose `deletedAt` the `deletedAt` timeline answers,
/// behind the v 1 instance the create recorded
/// Clause: 4.3.1, 4.5.8, 5.6.5, 5.7.3
/// Recorded: Prefer: respond-async on batch operations (IETF RFC 7240 section 4.1) queues the history the create
/// adds; a Core write of the same Entity that follows it is recorded after it, as clause 4.5.8 orders an
/// attribute's instances by the writes that made them
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0035_a_deletion_straight_after_a_respond_async_create_is_recorded() -> Result<()> {
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
    broker
        .entities()
        .delete_attribute(&sensor_id, &"v".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    // The deletion is recorded with its `deletedAt` and a null value (clause 4.5.8).
    let deleted = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .time_property(TemporalProperty::DeletedAt)
        .send()
        .await?;
    expect_that!(
        deleted.body(),
        has_evolution("v", all!(len(eq(1)), contains(any_attribute().null_value().with_member(Member::DeletedAt))))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("v", contains(property().value(1))));
    instance_id_where(evolution, &"v".parse()?, property().value(1))?;
    Ok(())
}
