//! A response without a request context names the versioned core context.

use andromeda_test_suite::{broker, core_context::core_entry, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity as application/ld+json with no Link header answers an @context that is the versioned core
/// context URL as one string
/// Clause: 5.5.5, 5.7.1, 6.3.6
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_9005_a_response_without_a_request_context_names_the_versioned_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_core_context_id = namespace.urn("Device", "core-context")?;
    let device_type = namespace.entity_type("Device")?;
    let device_core_context = Entity::builder()
        .id(device_core_context_id.clone())
        .entity_type(device_type)
        .property("temp", 21)
        .build()?;
    broker.entities().create(&device_core_context).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_core_context_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    Ok(())
}
