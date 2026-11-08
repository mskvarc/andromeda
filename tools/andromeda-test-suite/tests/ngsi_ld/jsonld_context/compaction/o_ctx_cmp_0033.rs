//! A post query under an inline import of an older core release is replaced by the current one last.

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
/// definition importing the protected v1.8 core beside its terms names those terms followed by the v1.9 core on the
/// Entity they answer
/// Clause: 5.2.3, 4.4, 5.7.2, 5.7.4, Annex C.8
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
/// Recorded: clause 4.4 names one core @context; Andromeda reads every embedded older release and the versionless
/// URL as it and names only v1.9
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0033_a_post_query_under_an_inline_import_of_an_older_core_release_is_replaced_by_the_current_one_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_body_context = tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed", "@import": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}}"#)
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
