//! The same compound request twice links one wrapper.

use andromeda_test_suite::{
    broker,
    minted_wrapper::{linked_context, listed_wrapper_ids, minted_wrapper},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Two json Retrieve Entity requests and a json Query Entities under the same user @context without the core
/// link the same wrapper, and the tenant holds exactly that one
/// Clause: 6.3.6, 5.13.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0010_the_same_compound_request_twice_links_one_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
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
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    let headed_2 = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(linked_context(&headed_2.head).ok(), some(eq(&wrapper.url)));
    let page = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(linked_context(&page.head).ok(), some(eq(&wrapper.url)));
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed)?, elements_are![eq(&wrapper.id)]);
    Ok(())
}
