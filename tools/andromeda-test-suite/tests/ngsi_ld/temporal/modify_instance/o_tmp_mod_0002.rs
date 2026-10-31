//! Keeps a decimal beyond binary64 precision exactly.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: an instance replaced by Modify Attribute instance with a fraction beyond binary64 precision reads back
/// digit for digit
/// Clause: 5.6.14.4, 4.5.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers, so the spec
/// mandates none; Andromeda's own requirement is arbitrary precision
#[gtest]
#[tokio::test]
async fn o_tmp_mod_0002_keeps_a_decimal_beyond_binary64_precision_exactly() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(120))?;
    let instance = Attribute::from(
        Property::builder(Value::Number("0.170000000000000005".parse()?))
            .observed_at("2020-09-01T12:03:00Z".parse()?)
            .build()?,
    );
    let received = broker
        .with_responses()
        .temporal()
        .modify_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await?;
    expect_that!(received.body.len(), eq(0));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("0.170000000000000005"));
    let retrieved_2 = &received_2.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(2))));
    Ok(())
}
