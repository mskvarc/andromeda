//! Adding attributes through the temporal api is recorded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution of two v instances and one w instance answers 204, and the Temporal
/// Evolution holds the four v instances and the w instance
/// Clause: 4.3.1, 5.6.12, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0023_adding_attributes_through_the_temporal_api_is_recorded() -> Result<()> {
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
        .property("v", Property::builder(3).observed_at("2026-07-01T00:02:00Z".parse()?))
        .property("v", Property::builder(7).observed_at("2026-07-01T00:03:00Z".parse()?))
        .property("w", Property::builder(1).observed_at("2026-07-01T00:03:00Z".parse()?))
        .build()?
        .attributes;
    broker_not_recorded.temporal().add_attributes(&counter_id, &instances).send().await?;
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
                len(eq(4)),
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
                ),
                contains(
                    any_attribute()
                        .value(3)
                        .observed_at("2026-07-01T00:02:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(7)
                        .observed_at("2026-07-01T00:03:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "w",
            all!(
                len(eq(1)),
                contains(
                    any_attribute()
                        .value(1)
                        .observed_at("2026-07-01T00:03:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    Ok(())
}
