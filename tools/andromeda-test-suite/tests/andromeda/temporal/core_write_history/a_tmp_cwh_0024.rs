//! A newer temporal instance still becomes current state.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::q,
};

/// Pins: Add Attributes to Temporal Evolution of a v instance newer than current state answers 204, current state
/// then holds it with its `observedAt`, and q=v==42 selects the Entity
/// Clause: 4.3.1, 4.5.5, 5.6.12, 5.7.1, 5.7.2
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0024_a_newer_temporal_instance_still_becomes_current_state() -> Result<()> {
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
        .property("v", Property::builder(42).observed_at("2026-07-01T00:10:00Z".parse()?))
        .build()?
        .attributes;
    broker_not_recorded.temporal().add_attributes(&counter_id, &instances).send().await?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&counter_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("v", any_attribute().value(42).observed_at("2026-07-01T00:10:00Z".parse()?))
    );
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&counter_type)
        .q(q::attr("v").eq(42))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&counter_id)));
    Ok(())
}
