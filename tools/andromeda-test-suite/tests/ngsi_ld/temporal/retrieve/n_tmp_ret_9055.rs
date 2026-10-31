//! A read on modified at answers the instances without observed at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};

/// Pins: speed holding 10, 20 and 30 observed hourly and 1000 and 2000 with no observedAt, read with timeproperty
/// modifiedAt and no lastN, answers 200 with all five instances, the broker stamping each with the modifiedAt it is read
/// by
/// Clause: 4.8, 4.11, 5.7.3.4, Table 6.19.3.1-1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9055_a_read_on_modified_at_answers_the_instances_without_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1000)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", 2000)
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(5)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(30)),
                contains(any_attribute().value(1000).without(Member::ObservedAt).with_member(Member::ModifiedAt)),
                contains(any_attribute().value(2000).without(Member::ObservedAt).with_member(Member::ModifiedAt))
            )
        )
    );
    Ok(())
}
