//! A json body without context is created under the core context.

use andromeda_test_suite::{attribute_matcher::property, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution sent as application/json with no @context and no Link header is created under the
/// core @context and reads back with its instance
/// Clause: 5.6.11.4, 6.3.5, 6.18.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0002_a_json_body_without_context_is_created_under_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_id));
    expect_that!(evolution.entity_type, eq(&OneOrMany::one(vehicle_type.clone())));
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, property().value(120).observed_at("2020-08-01T12:03:00Z".parse()?)))
        )
    );
    Ok(())
}
