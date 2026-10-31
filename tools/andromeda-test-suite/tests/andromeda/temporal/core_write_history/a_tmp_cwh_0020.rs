//! Scope queries follow an unrecorded scope.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{EntityFragment, NgsiLdMediaType, OneOrMany, Property, Scope, ScopeChange, evolution::TemporalEntity},
    query::scope_query::ScopeQuery,
};

/// Pins: after an unrecorded Merge Entity setting scope `/CoreWriteHistory/North` and then
/// `/CoreWriteHistory/South`, `scopeQ` selects the Entity by the scope it holds and not by the other, while the
/// Temporal Evolution holds only the seeded v instances and no scope
/// Clause: 4.3.1, 4.18, 4.19, 5.6.17, 5.7.2
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0020_scope_queries_follow_an_unrecorded_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/CoreWriteHistory/North".parse::<Scope>()?)))
        .build()?;
    broker_not_recorded.entities().merge(&sensor_id, &fragment).send().await?.done()?;
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .scope_q("/CoreWriteHistory/North".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .scope_q("/CoreWriteHistory/South".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(0)));
    expect_that!(page_2.items(), len(eq(0)));
    let fragment_2 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/CoreWriteHistory/South".parse::<Scope>()?)))
        .build()?;
    broker_not_recorded.entities().merge(&sensor_id, &fragment_2).send().await?.done()?;
    let page_3 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .scope_q("/CoreWriteHistory/South".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(1)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_id)));
    let page_4 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .scope_q("/CoreWriteHistory/North".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(0)));
    expect_that!(page_4.items(), len(eq(0)));
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, is_empty());
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(
                    any_attribute()
                        .value(1)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(2)
                        .observed_at("2026-07-01T00:01:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    Ok(())
}
