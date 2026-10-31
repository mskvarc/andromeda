//! Keeps numbers beyond binary64 precision exactly.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution whose instances hold a fraction and an integer beyond binary64 precision is created
/// and reads back with both numbers digit for digit
/// Clause: 5.6.11.4, 4.5.1
/// Recorded: IETF RFC 8259 clause 6 lets an implementation limit the range and precision of numbers, so the spec
/// mandates none; Andromeda's own requirement is arbitrary precision
#[gtest]
#[tokio::test]
async fn o_tmp_ups_0005_keeps_numbers_beyond_binary64_precision_exactly() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(Value::Number("0.100000000000000005".parse()?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("18446744073709551616".parse()?)).observed_at("2020-08-01T12:05:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("0.100000000000000005"));
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("18446744073709551616"));
    let retrieved = &received.outcome;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    Ok(())
}
