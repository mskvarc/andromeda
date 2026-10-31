//! An ordinary user context is used.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity whose body @context is the core context, a remote context defining axle and an inline
/// definition of gear answers 201, and Retrieve Entity under a Link naming the remote context answers axle with its
/// value
/// Clause: 5.5.7, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9046_an_ordinary_user_context_is_used() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            contexts.reference("contexts/axle.jsonld")?,
            ContextEntry::Inline(format!(r#"{{"gear": "https://example.org/{namespace}/gear"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_ordinary_id = namespace.urn("Device", "ordinary")?;
    let device_type = namespace.entity_type("Device")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/axle.jsonld")?))?;
    contexts
        .serve(
            "contexts/axle.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "axle": "https://example.org/{namespace}/axle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device_ordinary = Entity::builder()
        .id(device_ordinary_id.clone())
        .entity_type(device_type)
        .property("gear", 3)
        .property("axle", 2)
        .build()?;
    body_context.entities().create(&device_ordinary).send().await?.done()?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_ordinary_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("axle", any_attribute().value(2)));
    Ok(())
}
