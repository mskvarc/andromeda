//! A post query under the core context named twice is named once.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
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
    model::{Context, Entity, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Entities and Query Temporal Evolution of Entities by POST as application/ld+json under the core, a
/// user @context and the core again names the user @context followed by the core once on the Entity they answer
/// Clause: 5.2.3, 4.4, 5.7.2, 5.7.4, Annex C.8
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0035_a_post_query_under_the_core_context_named_twice_is_named_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            core_entry()?,
            contexts.reference("jc/user/terms.jsonld")?,
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
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
    let page = tenant_body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let item = page.items().first().or_fail()?;
    expect_that!(item.context.as_ref().is_some_and(|context| matches!(context, Context::Multiple(_))), eq(true));
    expect_that!(item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(2)), none());
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item.context.as_ref().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    expect_that!(page.items(), len(le(1)));
    expect_that!(page.items(), nth(0, has_id(&device_id)));
    let page_2 = tenant_body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(le(1)));
    let item_2 = page_2.items().first().or_fail()?;
    expect_that!(item_2.context.as_ref().is_some_and(|context| matches!(context, Context::Multiple(_))), eq(true));
    expect_that!(item_2.context.as_ref().map(Context::entries).and_then(|entries| entries.get(2)), none());
    expect_that!(
        item_2.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/user/terms.jsonld")?))
    );
    expect_that!(
        item_2.context.as_ref().map(Context::entries).and_then(|entries| entries.get(1)),
        some(eq(&core_entry()?))
    );
    expect_that!(page_2.items(), nth(0, has_id(&device_id)));
    Ok(())
}
