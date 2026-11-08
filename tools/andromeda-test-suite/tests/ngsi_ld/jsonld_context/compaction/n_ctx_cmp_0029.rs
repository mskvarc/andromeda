//! A post query under an inline definition without the core context is followed by it.

use andromeda_test_suite::{
    broker,
    context_terms::{defines_terms, listed_entries},
    core_context::core_entry,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Query Entities and Query Temporal Evolution of Entities by POST as application/ld+json under an inline
/// definition without the core @context names it followed by the v1.9 core on the Entity they answer
/// Clause: 5.2.3, 4.4, 5.7.2, 5.7.4
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0029_a_post_query_under_an_inline_definition_without_the_core_context_is_followed_by_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_body_context = tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed"}}"#)
                .parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "{device_iri}",
        "speed": "{speed_iri}"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let page = tenant_body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&device_id)));
    let item = page.items().first().or_fail()?;
    expect_that!(
        item.context.as_ref().and_then(listed_entries),
        some(elements_are![
            defines_terms([(format!("Device_{namespace}").as_str(), device_iri.as_str()), ("speed", speed_iri.as_str())]),
            eq(&core_entry()?)
        ])
    );
    let page_2 = tenant_body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&device_id)));
    let item_2 = page_2.items().first().or_fail()?;
    expect_that!(
        item_2.context.as_ref().and_then(listed_entries),
        some(elements_are![
            defines_terms([(format!("Device_{namespace}").as_str(), device_iri.as_str()), ("speed", speed_iri.as_str())]),
            eq(&core_entry()?)
        ])
    );
    Ok(())
}
