//! The default records every core write that modifies current state.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{BatchUpsert, UpsertMode},
    },
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: on the broker defaults, Create Entity and Update Attributes each add a v instance to the Temporal
/// Evolution, a Batch Entity Upsert restating the reading adds none, and Delete Attribute adds one deletion
/// Clause: 4.3.1, 4.5.8, 5.8.6, 5.6.1, 5.6.2, 5.6.5, 5.6.8, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); Andromeda records Core writes by default, each
/// for the attribute instances it modified (a change as clause 5.8.6 defines it), so a restatement records nothing
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0027_the_default_records_every_core_write_that_modifies_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let v = "v".parse::<AttributeName>()?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let restated = [Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?];
    let upsert = broker
        .send_unchecked(broker.unchecked(&BatchUpsert::builder().entities(&restated).mode(UpsertMode::Update).build())?)
        .await;
    expect_that!(&upsert, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(property().value(1).observed_at("2026-07-01T00:00:00Z".parse()?)),
                contains(property().value(2).observed_at("2026-07-01T00:01:00Z".parse()?))
            )
        )
    );
    instance_id_where(evolution, &"v".parse()?, property().value(1).observed_at("2026-07-01T00:00:00Z".parse()?))?;
    instance_id_where(evolution, &"v".parse()?, property().value(2).observed_at("2026-07-01T00:01:00Z".parse()?))?;
    broker.entities().delete_attribute(&sensor_id, &v).send().await?.done()?;
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
    Ok(())
}
