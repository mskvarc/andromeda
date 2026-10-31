//! Deleting a temporal evolution still applies.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    evolution_matcher::{has_evolution, lacks_evolution},
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

/// Pins: Delete Attribute from Temporal Evolution of w answers 204 and leaves the v instances, and Delete Temporal
/// Evolution of the Entity answers 204 and Retrieve Temporal Evolution then 404
/// Clause: 4.3.1, 5.6.13, 5.6.16, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0026_deleting_a_temporal_evolution_still_applies() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let counter_id = namespace.urn("Counter", 1)?;
    let counter_type = namespace.entity_type("Counter")?;
    let counter_evolution = TemporalEntity::builder()
        .id(counter_id.clone())
        .entity_type(counter_type.clone())
        .property("v", Property::builder(4).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(6).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&counter_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(counter_id.clone())
        .entity_type(counter_type.clone())
        .property("w", Property::builder(1).observed_at("2026-07-01T00:03:00Z".parse()?))
        .build()?
        .attributes;
    broker_not_recorded.temporal().add_attributes(&counter_id, &instances).send().await?;
    broker_not_recorded
        .temporal()
        .delete_attribute(&counter_id, &"w".parse::<AttributeName>()?)
        .send()
        .await?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&counter_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(
                    any_attribute()
                        .value(4)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(6)
                        .observed_at("2026-07-01T00:01:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    expect_that!(evolution, lacks_evolution("w"));
    broker_not_recorded.temporal().delete(&counter_id).send().await?;
    let answer = broker_not_recorded
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&counter_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
