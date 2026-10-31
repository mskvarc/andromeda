//! Formatting does not split a wrapper and array order does.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    minted_wrapper::{listed_wrapper_ids, minted_wrapper},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{
        ContextDelivery,
        ResponseHead,
        headers::Accept,
        operation::{ContextKindFilter, QueryMethod},
    },
    model::{Context, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Entities by POST answered as json links the same wrapper for inline definitions differing only in
/// member order and whitespace, and different wrappers for the same two URLs in either order; the tenant then holds
/// three wrappers
/// Clause: 6.3.6, 5.13.1, JSON-LD 1.1 s4.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0011_formatting_does_not_split_a_wrapper_and_array_order_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let core = config.core_context();
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/terms.jsonld")?,
            contexts.reference("jc/user/importing.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Multiple(vec![
            contexts.reference("jc/user/importing.jsonld")?,
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
    // The spacing and member order of the inline definition are the subject, and the typed context rewrites both.
    let compact = format!(
        r#"{{"type": "Query", "entities": [{{"type": "https://test.andromeda/{namespace}/Device", "id": "urn:ngsi-ld:Device:{namespace}-1"}}], "@context": {{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed"}}}}"#
    );
    let request = request_to(&tenant, Method::POST, "entityOperations/query")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .body(Bytes::from(compact));
    let answer = tenant.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let wrapper = minted_wrapper(&ResponseHead::new(answer.headers().clone()))?;
    let spaced = format!(
        "{{\"type\": \"Query\", \"entities\": [{{\"type\": \"https://test.andromeda/{namespace}/Device\", \"id\": \"urn:ngsi-ld:Device:{namespace}-1\"}}], \"@context\": {{ \"speed\" :\"https://test.andromeda/{namespace}/speed\",\n\n  \"Device_{namespace}\":\"https://test.andromeda/{namespace}/Device\" }}}}"
    );
    let request_2 = request_to(&tenant, Method::POST, "entityOperations/query")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .body(Bytes::from(spaced));
    let answer_2 = tenant.send_unchecked(request_2).await?;
    expect_that!(answer_2.status(), eq(StatusCode::OK));
    let wrapper_2 = minted_wrapper(&ResponseHead::new(answer_2.headers().clone()))?;
    expect_that!(wrapper_2.url, eq(&wrapper.url));
    let headed = tenant_body_context
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let wrapper_3 = minted_wrapper(&headed.head)?;
    let headed_2 = tenant_body_context_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    let wrapper_4 = minted_wrapper(&headed_2.head)?;
    expect_that!(wrapper_4.url, not(eq(&wrapper_3.url)));
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed)?, len(eq(3)));
    Ok(())
}
