//! An observed at of six fraction digits reads back exactly.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of inspected observed at 2026-03-04T05:06:07.123456Z reads back that instance
/// with `observedAt` 2026-03-04T05:06:07.123456Z
/// Clause: 4.6.3, 4.5.7, 5.6.11, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9041_an_observed_at_of_six_fraction_digits_reads_back_exactly() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_six_digit_id = namespace.urn("Vehicle", "six-digit")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_six_digit_evolution = TemporalEntity::builder()
        .id(vehicle_six_digit_id.clone())
        .entity_type(vehicle_type)
        .property(
            "inspected",
            Property::builder(Value::from("2026-03-04T05:06:07.123456Z".parse::<DateTime>()?)).observed_at("2026-03-04T05:06:07.123456Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_six_digit_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_six_digit_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "inspected",
            all!(len(eq(1)), nth(0, any_attribute().observed_at("2026-03-04T05:06:07.123456Z".parse()?)))
        )
    );
    Ok(())
}
