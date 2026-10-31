//! A temporal post query answered as json names one context link.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    minted_wrapper::{linked_context, minted_wrapper},
    namespace::Namespace,
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

/// Pins: Query Temporal Evolution of Entities by POST answered as json links its body @context itself when it is one
/// URL ending with the core @context, and one hosted wrapper when it is an array of two URLs
/// Clause: 6.3.6, 5.7.4, 5.13.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
/// Recorded: Andromeda links a request @context that is one URL leaving the core @context in force last (Annex C.8)
/// as that URL, and hosts nothing for it
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0006_a_temporal_post_query_answered_as_json_names_one_context_link() -> Result<()> {
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
    let tenant_body_context = tenant
        .with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
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
          "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
          "speed": "https://test.andromeda/{namespace}/speed"
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
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(linked_context(&headed.head).ok(), some(eq(&contexts.url("jc/user/wrapper.jsonld")?)));
    let headed_2 = tenant_body_context_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    minted_wrapper(&headed_2.head)?;
    Ok(())
}
