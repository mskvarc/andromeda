//! A core upsert restating the newest reading does not record it twice.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{
        Entity,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
};

/// Pins: a Batch Entity Upsert with options=update restating the newest of four readings answers 204, the Temporal
/// Evolution holds the four readings once each, and the aggregated `totalCount` over P1D is 4 and the sum 20
/// Clause: 4.3.1, 5.6.8, 5.7.3, 4.5.19
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0022_a_core_upsert_restating_the_newest_reading_does_not_record_it_twice() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let counter_id = namespace.urn("Counter", 1)?;
    let counter_type = namespace.entity_type("Counter")?;
    let counter_evolution = TemporalEntity::builder()
        .id(counter_id.clone())
        .entity_type(counter_type.clone())
        .property("v", Property::builder(4).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(6).observed_at("2026-07-01T00:01:00Z".parse()?))
        .property("v", Property::builder(3).observed_at("2026-07-01T00:02:00Z".parse()?))
        .property("v", Property::builder(7).observed_at("2026-07-01T00:03:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&counter_evolution).send().await?.created()?;
    let entities = [Entity::builder()
        .id(counter_id.clone())
        .entity_type(counter_type.clone())
        .property("v", Property::builder(7).observed_at("2026-07-01T00:03:00Z".parse()?))
        .build()?];
    broker_not_recorded
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Update)
        .send()
        .await?
        .done()?;
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
    let retrieved_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&counter_id)
        .aggregation_methods("totalCount,sum".parse()?)
        .aggregation_period("P1D".parse()?)
        .send()
        .await?;
    let aggregated = retrieved_2.body();
    expect_that!(aggregated, has_aggregate("v", AggregationMethod::TotalCount, nth(0, period().value(4))));
    expect_that!(aggregated, has_aggregate("v", AggregationMethod::Sum, nth(0, period().value(20))));
    let retrieved_3 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&counter_id)
        .send()
        .await?;
    let entity = retrieved_3.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(7)));
    Ok(())
}
