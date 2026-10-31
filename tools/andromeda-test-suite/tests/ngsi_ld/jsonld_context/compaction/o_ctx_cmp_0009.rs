//! A compound or inline body context is linked through a wrapper of the body value.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    context_terms::{defines_terms, listed_entries},
    core_context::core_entry,
    minted_wrapper::{minted_wrapper, served_document},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Entities by POST answered as json under a body @context of two URLs, of an inline definition, or of a
/// URL followed by the core links one wrapper holding exactly the value an application/ld+json body would carry
/// Clause: 6.3.6, 5.13.1, Annex C.8
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0009_a_compound_or_inline_body_context_is_linked_through_a_wrapper_of_the_body_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let device_term = format!("Device_{namespace}");
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"{device_term}": "{device_iri}", "speed": "{speed_iri}"}}"#).parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_3 = tenant
        .with_context(Context::Multiple(vec![contexts.reference("jc/user/wrapper.jsonld")?, core_entry()?]))?
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
    contexts
        .serve(
            "jc/user/wrapper.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/user/importing.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{core}",
        {{
          "{device_term}": "{device_iri}",
          "speed": "{speed_iri}"
        }}
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_body_context
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
            core_entry()?
        ]))
    );
    let headed_2 = tenant_body_context_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let wrapper_2 = minted_wrapper(&headed_2.head)?;
    let document_2 = served_document(&tenant, &wrapper_2).await?;
    expect_that!(document_2.extra_members, is_empty());
    expect_that!(
        listed_entries(&document_2.context),
        some(elements_are![
            defines_terms([(device_term.as_str(), device_iri.as_str()), ("speed", speed_iri.as_str())]),
            eq(&core_entry()?)
        ])
    );
    let headed_3 = tenant_body_context_3
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(device_iri.as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let wrapper_3 = minted_wrapper(&headed_3.head)?;
    let document_3 = served_document(&tenant, &wrapper_3).await?;
    expect_that!(document_3.extra_members, is_empty());
    expect_that!(
        document_3.context,
        eq(&Context::Multiple(vec![contexts.reference("jc/user/wrapper.jsonld")?, core_entry()?]))
    );
    Ok(())
}
