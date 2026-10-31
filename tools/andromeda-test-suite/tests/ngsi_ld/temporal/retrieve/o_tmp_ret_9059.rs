//! Last n skips the instances core api writes record without observed at.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: speed created by Create Entity as 1 observed at 2024-01-01T00:00:00Z and replaced by Update Attributes
/// with 2 (no `observedAt`), 3 (observed at 02:00) and 4 (no `observedAt`) answers `lastN` 1 with 3, `lastN` 10 and
/// a read without `lastN` with 1 and 3, and a read on `modifiedAt` with all four instances
/// Clause: 5.7.3.4, 4.3.1, 4.5.7, 4.8, 5.6.2
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and an attribute written without `observedAt` is
/// recorded without one, which a read on `observedAt` leaves out and a read on `modifiedAt` answers
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9059_last_n_skips_the_instances_core_api_writes_record_without_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder()
        .property("speed", Property::builder(3).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment_2).send().await?.done()?;
    let fragment_3 = EntityFragment::builder().property("speed", 4).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment_3).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(1)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(3).observed_at("2024-01-01T02:00:00Z".parse()?)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(10)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(1)), nth(1, any_attribute().value(3))))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_3 = retrieved_3.body();
    expect_that!(
        evolution_3,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(1)), nth(1, any_attribute().value(3))))
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution_4 = retrieved_4.body();
    expect_that!(
        evolution_4,
        has_evolution(
            "speed",
            all!(
                len(eq(4)),
                contains(any_attribute().value(1)),
                contains(any_attribute().value(2)),
                contains(any_attribute().value(3)),
                contains(any_attribute().value(4)),
                contains(property().value(2)),
                contains(any_attribute().value(2)).times(eq(1)),
                not(contains(any_attribute().value(2).with_member(Member::ObservedAt))),
                contains(property().value(4)),
                contains(any_attribute().value(4)).times(eq(1)),
                not(contains(any_attribute().value(4).with_member(Member::ObservedAt)))
            )
        )
    );
    Ok(())
}
