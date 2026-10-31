//! Last n on modified at ranks the instances without observed at by their write.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};

/// Pins: speed written with 10, 20 and 30 observed hourly, then 1000 and then 2000 each added with no `observedAt`
/// by a later Add Attributes, answers `lastN` 2 on timeproperty `modifiedAt` with 1000 and 2000, and `lastN` 2 on
/// the default `observedAt` with 20 and 30
/// Clause: 5.7.3.4, 4.8, 4.11, 5.6.12.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9058_last_n_on_modified_at_ranks_the_instances_without_observed_at_by_their_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1000)
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_id, &instances).send().await?;
    let instances_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2000)
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_id, &instances_2).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .last_n(2)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(2)), nth(0, any_attribute().value(1000)), nth(1, any_attribute().value(2000)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(2)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(20)), nth(1, any_attribute().value(30))))
    );
    Ok(())
}
