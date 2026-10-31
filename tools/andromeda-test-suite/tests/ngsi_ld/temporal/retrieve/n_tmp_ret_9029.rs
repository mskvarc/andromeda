//! Under the declaring context terms compact.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of an Entity under the @context declaring its type, speed and `fuelLevel`
/// answers the short type and both Attributes under their terms and no full IRI
/// Clause: 5.7.3, 5.5.7
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9029_under_the_declaring_context_terms_compact() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/vehicle")?))?;
    let vehicle_with_context_id = namespace.urn("Vehicle", "with-context")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    contexts
        .serve(
            "contexts/vehicle",
            &format!(
                r#"{{
      "@context": {{
        "Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle",
        "speed": "https://test.andromeda/{namespace}/speed",
        "fuelLevel": "https://test.andromeda/{namespace}/fuelLevel"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let vehicle_with_context_evolution = TemporalEntity::builder()
        .id(vehicle_with_context_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&vehicle_with_context_evolution).send().await?.created()?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_with_context_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_with_context_id));
    expect_that!(evolution.entity_type, eq(&OneOrMany::one(vehicle_type.clone())));
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    expect_that!(evolution, has_evolution("fuelLevel", len(eq(1))));
    expect_that!(evolution, lacks_evolution(&format!("https://test.andromeda/{namespace}/speed")));
    broker.temporal().delete(&vehicle_with_context_id).send().await?;
    Ok(())
}
