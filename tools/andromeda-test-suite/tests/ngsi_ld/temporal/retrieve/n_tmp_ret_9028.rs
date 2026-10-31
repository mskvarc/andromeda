//! Without a context undeclared terms stay full iris.

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
    model::{Context, EntityTypeName, NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of an Entity written under a @context declaring its type, speed and
/// `fuelLevel`, with no @context, answers the type and both Attributes as their full IRIs and no short term
/// Clause: 5.7.3, 5.5.7
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9028_without_a_context_undeclared_terms_stay_full_iris() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/vehicle")?))?;
    let vehicle_no_context_id = namespace.urn("Vehicle", "no-context")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_type_2 = format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?;
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
    let vehicle_no_context_evolution = TemporalEntity::builder()
        .id(vehicle_no_context_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&vehicle_no_context_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_no_context_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_no_context_id));
    expect_that!(evolution.entity_type, eq(&OneOrMany::one(vehicle_type_2)));
    expect_that!(evolution, has_evolution(&format!("https://test.andromeda/{namespace}/speed"), len(eq(2))));
    expect_that!(evolution, has_evolution(&format!("https://test.andromeda/{namespace}/fuelLevel"), len(eq(1))));
    expect_that!(evolution, lacks_evolution("speed"));
    expect_that!(evolution, lacks_evolution("fuelLevel"));
    broker.temporal().delete(&vehicle_no_context_id).send().await?;
    Ok(())
}
