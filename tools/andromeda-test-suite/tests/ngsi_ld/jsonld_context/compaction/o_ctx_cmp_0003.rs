//! Body less answers name no context and host nothing.

use andromeda_test_suite::{broker, header_matcher::absent, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::{Accept, Link},
        operation::ContextKindFilter,
    },
    model::{Context, Entity, EntityFragment},
};

/// Pins: Create Entity (201), Update Attributes (204) and Delete Entity (204) under a user @context without the
/// core answer with no Link, and the tenant holds no wrapper afterwards
/// Clause: 6.3.6, 5.13.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps
/// them the tenant's own)
/// Recorded: Andromeda hosts a wrapper only for an answer that names its @context in a Link and has a body
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0003_body_less_answers_name_no_context_and_host_nothing() -> Result<()> {
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
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    let received = tenant_linked.with_responses().entities().create(&device).send().await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<Link>(), absent());
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    let received_2 = tenant_linked
        .with_responses()
        .entities()
        .update_attributes(&device_id, &fragment)
        .send()
        .await?;
    received_2.outcome.done()?;
    expect_that!(received_2.head.typed::<Link>(), absent());
    let received_3 = tenant_linked.with_responses().entities().delete(&device_id).send().await?;
    received_3.outcome.done()?;
    expect_that!(received_3.head.typed::<Link>(), absent());
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(contexts, each(not(displays_as(contains_substring("JsonLdContext")))));
    Ok(())
}
