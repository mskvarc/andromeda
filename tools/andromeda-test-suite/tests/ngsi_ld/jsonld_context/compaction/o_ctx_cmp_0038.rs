//! A body rendered under a body context reads as the stored entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    context_terms::{defines_terms, listed_entries},
    core_context::core_entry,
    entity_matcher::{has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Entities by POST as application/ld+json under an inline definition, and under the v1.8 core followed
/// by a user @context, answers the short names under a @context that expands them to the stored IRIs
/// Clause: 4.4, 5.2.3, 5.7.2
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
/// Recorded: the Rust case hands the body to an independent JSON-LD 1.1 processor; the file pins everything that
/// fixes what such a processor reads: the exact @context named, the documents behind it (served by the file), the
/// short names in the body, and the IRIs stored
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0038_a_body_rendered_under_a_body_context_reads_as_the_stored_entity() -> Result<()> {
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
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let device_term = format!("Device_{namespace}");
    let stored_type = device_iri.parse::<EntityTypeName>()?;
    let tenant_body_context = tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"{device_term}": "{device_iri}", "speed": "{speed_iri}"}}"#).parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned())?),
            contexts.reference("jc/user/terms.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "{device_term}": "{device_iri}",
        "speed": "{speed_iri}"
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
        .property("colour", "red")
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(stored_type)));
    expect_that!(entity, has_attribute(&speed_iri, any_attribute().value(3)));
    expect_that!(entity, has_attribute("colour", any_attribute().value("red")));
    expect_that!(member_count(entity), eq(4));
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
    let item = page.items().first().or_fail()?;
    expect_that!(item.entity_type, eq(&OneOrMany::one(device_type.clone())));
    expect_that!(item, has_attribute("speed", any_attribute().value(3)));
    expect_that!(item, has_attribute("colour", any_attribute().value("red")));
    expect_that!(member_count(item), eq(5));
    expect_that!(
        item.context.as_ref().and_then(listed_entries),
        some(elements_are![
            defines_terms([(device_term.as_str(), device_iri.as_str()), ("speed", speed_iri.as_str())]),
            eq(&core_entry()?)
        ])
    );
    let page_2 = tenant_body_context_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let item_2 = page_2.items().first().or_fail()?;
    expect_that!(item_2.entity_type, eq(&OneOrMany::one(device_type)));
    expect_that!(item_2, has_attribute("speed", any_attribute().value(3)));
    expect_that!(item_2, has_attribute("colour", any_attribute().value("red")));
    expect_that!(member_count(item_2), eq(5));
    expect_that!(
        item_2.context.as_ref().and_then(listed_entries),
        some(elements_are![eq(&contexts.reference("jc/user/terms.jsonld")?), eq(&core_entry()?)])
    );
    Ok(())
}
