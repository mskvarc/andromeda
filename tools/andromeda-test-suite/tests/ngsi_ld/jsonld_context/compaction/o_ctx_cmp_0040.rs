//! A context named again after another is rendered and processed again.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A create under `[A, B, A]` stores the term both define under A's IRI, and a POST query under `[A, B, A]`
/// names it with its repeat followed by the core
/// Clause: 5.5.7, 5.2.3, JSON-LD 1.1 API s4.1.2
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0040_a_context_named_again_after_another_is_rendered_and_processed_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/second.jsonld")?,
            contexts.reference("jc/user/terms.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
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
    contexts
        .serve(
            "jc/user/second.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/other/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 4)
        .build()?;
    tenant_body_context.entities().create(&device).send().await?.done()?;
    let page = tenant_body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let item = page.items().first().or_fail()?;
    expect_that!(item.context.as_ref().is_some_and(|context| matches!(context, Context::Multiple(_))), eq(true));
    expect_that!(item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(4)), none());
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&contexts.reference("jc/user/second.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(2)),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(3)),
        some(eq(&core_entry()?))
    );
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute(&format!("https://test.andromeda/{namespace}/other/speed")));
    expect_that!(
        entity,
        has_attribute(&format!("https://test.andromeda/{namespace}/speed"), any_attribute().value(4))
    );
    Ok(())
}
