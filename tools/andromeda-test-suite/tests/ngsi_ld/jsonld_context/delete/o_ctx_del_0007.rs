//! Delete the core context url is refused and resolution stays.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{ContextId, Entity, EntityTypeName, ErrorType, NgsiLdMediaType},
};

/// Pins: Delete @context of the core @context answers 404 `ResourceNotFound`, and an entity answer still names the core
/// @context in its Link, so resolution is untouched
/// Clause: 5.13.5.4, 6.3.6, 4.4
/// Tenant: private (the case addresses the core @context URL and reads back this tenant's entity; clause 4.14 keeps
/// each tenant's stored @contexts its own)
/// Recorded: clause 5.13.5.4 raises `ResourceNotFound` for an identifier no @context storage holds; the core @context is
/// not a stored entry, and the broker resolves it from its embedded copy
#[gtest]
#[tokio::test]
async fn o_ctx_del_0007_delete_the_core_context_url_is_refused_and_resolution_stays() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant.entities().create(&device).send().await?.done()?;
    let core_id = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".parse::<ContextId>()?;
    let answer = tenant.with_responses().contexts().delete(&core_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.context_urls().any(|url| url.as_str() == config.core_context().as_str())),
        some(eq(true))
    );
    Ok(())
}
