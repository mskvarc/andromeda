//! Modifying and deleting a recorded instance still apply.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance sets the first v instance to 99 and Delete Attribute instance removes the
/// second, each answering 204, and the Temporal Evolution then holds the one instance 99
/// Clause: 4.3.1, 5.6.14, 5.6.15, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0025_modifying_and_deleting_a_recorded_instance_still_apply() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let counter_id = namespace.urn("Counter", 1)?;
    let counter_type = namespace.entity_type("Counter")?;
    let counter_evolution = TemporalEntity::builder()
        .id(counter_id.clone())
        .entity_type(counter_type)
        .property("v", Property::builder(4).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(6).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&counter_evolution).send().await?.created()?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&counter_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("v", len(eq(2))));
    let first = instance_id_where(evolution, &"v".parse()?, any_attribute().value(4))?;
    let second = instance_id_where(evolution, &"v".parse()?, any_attribute().value(6))?;
    let instance = Attribute::from(Property::builder(99).observed_at("2026-07-01T00:00:00Z".parse()?).build()?);
    broker_not_recorded
        .temporal()
        .modify_instance(&counter_id, &"v".parse::<AttributeName>()?, &first, &instance)
        .send()
        .await?;
    broker_not_recorded
        .temporal()
        .delete_instance(&counter_id, &"v".parse::<AttributeName>()?, &second)
        .send()
        .await?;
    let retrieved_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&counter_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "v",
            all!(
                len(eq(1)),
                contains(
                    any_attribute()
                        .value(99)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    Ok(())
}
