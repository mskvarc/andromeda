//! A simplified values member creates one instance per pair.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution written in the simplified temporal representation is created with one Property
/// instance per `[value, observedAt]` pair of its values member
/// Clause: 5.2.20, 4.5.9, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0003_a_simplified_values_member_creates_one_instance_per_pair() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (expected a JSON object at speed.values), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "values": [[55, "2020-08-01T12:00:00Z"], [61, "2020-08-01T13:00:00Z"]]}"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_id));
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(property().value(55)),
                contains(any_attribute().value(55)).times(eq(1)),
                contains(any_attribute().value(55).observed_at("2020-08-01T12:00:00Z".parse()?)),
                contains(any_attribute().value(55)).times(eq(1)),
                contains(property().value(61)),
                contains(any_attribute().value(61)).times(eq(1)),
                contains(any_attribute().value(61).observed_at("2020-08-01T13:00:00Z".parse()?)),
                contains(any_attribute().value(61)).times(eq(1))
            )
        )
    );
    Ok(())
}
