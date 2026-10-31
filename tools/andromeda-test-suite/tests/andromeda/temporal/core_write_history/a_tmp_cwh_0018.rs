//! Q follows an unrecorded update.

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
    model::{EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::q,
};

/// Pins: after an unrecorded Update Attributes of v from 2 to 5, Query Entities selects the Entity by q=v==5 and
/// q=v>4 and no longer by q=v==2, while the Temporal Evolution holds only the seeded v instances
/// Clause: 4.3.1, 4.9, 5.6.2, 5.7.2
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0018_q_follows_an_unrecorded_update() -> Result<()> {
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
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").eq(2))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(5).observed_at("2026-07-01T00:02:00Z".parse()?))
        .build()?;
    broker_not_recorded.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let page_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").eq(5))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_id)));
    let page_3 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").eq(2))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(0)));
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("v").gt(4))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(1)));
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&sensor_id)));
    let retrieved = broker_not_recorded
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
