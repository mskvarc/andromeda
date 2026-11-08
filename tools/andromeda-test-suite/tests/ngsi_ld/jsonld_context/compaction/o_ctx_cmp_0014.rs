//! With no user context the one link names the core context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    link_matcher::link_value,
    minted_wrapper::{linked_context, listed_wrapper_ids},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::ContextKindFilter,
    },
    model::{Context, ContextEntry, Entity, NgsiLdMediaType},
};

/// Pins: A json Retrieve Entity with no Link, and with a Link naming the v1.8 core @context, links the v1.9 core
/// @context typed application/ld+json, and the tenant holds no wrapper
/// Clause: 6.3.6, 4.4, 5.13.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 4.4 names one core @context; Andromeda reads an older release as it and names v1.9 in every
/// answer
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0014_with_no_user_context_the_one_link_names_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_older_core = tenant.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned(),
    )?)))?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(linked_context(&headed.head)?.as_str(), eq(config.core_context().as_str()));
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(link_value(RelationType::JsonLdContext).with_media_type("application/ld+json"))
    );
    let headed_2 = tenant_older_core
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(linked_context(&headed_2.head)?.as_str(), eq(config.core_context().as_str()));
    expect_that!(
        headed_2.head.typed::<Link>().valid(),
        some(link_value(RelationType::JsonLdContext).with_media_type("application/ld+json"))
    );
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed)?, is_empty());
    Ok(())
}
