//! Formatting and member order do not split an implicit context.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use url::Url;

/// Pins: Two subscriptions whose @context arrays hold the same definitions with members in another order and other
/// whitespace name the same jsonldContext, and the tenant holds one wrapper
/// Clause: 5.13.1, 5.8.1, RFC 8785
/// Tenant: private (the case counts the `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them the
/// tenant's own)
/// Recorded: clause 5.13.1 leaves the format of a stored @context's local identifier to the broker; Andromeda names
/// every entry urn:ngsi-ld:JsonLdContext:<uuid>, a random (version 4) UUID for a Hosted or Cached entry and the
/// version 5 UUID of the canonical content for an `ImplicitlyCreated` one
#[gtest]
#[tokio::test]
async fn o_ctx_add_0012_formatting_and_member_order_do_not_split_an_implicit_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let probe_type = namespace.entity_type("IdentityProbe")?;
    let body_context = tenant
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(
                format!(
                    r#"{{"speed": "https://test.andromeda/{namespace}/speed", "IdentityProbe_{namespace}": "https://test.andromeda/{namespace}/IdentityProbe"}}"#
                )
                .parse::<InlineContext>()?,
            ),
            ContextEntry::Inline(
                format!(r#"{{"rate": {{"@id": "https://test.andromeda/{namespace}/rate", "@type": "@id"}}}}"#).parse::<InlineContext>()?,
            ),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    body_context.subscriptions().create(&subscription).send().await?;
    // The point is the byte form of the second context array (member order, spacing, a tab), which the model would
    // write in one canonical form, so the body goes as written.
    let body = format!(
        "{{\"id\": \"urn:ngsi-ld:Subscription:{namespace}-2\", \"type\": \"Subscription\", \"entities\": [{{\"type\": \"IdentityProbe_{namespace}\"}}], \"notification\": {{\"endpoint\": {{\"uri\": \"{unreachable}/notify\", \"accept\": \"application/json\"}}}}, \"@context\": [ {{\n  \"IdentityProbe_{namespace}\" :\"https://test.andromeda/{namespace}/IdentityProbe\",\n\t\"speed\":  \"https://test.andromeda/{namespace}/speed\" }} ,{{\"rate\":{{ \"@type\":\"@id\" , \"@id\" : \"https://test.andromeda/{namespace}/rate\"}}}}]}}"
    );
    let request = request_to(&tenant, Method::POST, "subscriptions")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(body));
    let answer = tenant.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let first = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let first_url = first.jsonld_context();
    expect_that!(first_url.map(Url::as_str), some(contains_substring("/ngsi-ld/v1/jsonldContexts/")));
    let second = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(second.jsonld_context(), eq(first_url));
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    HostedContext::listed(&contexts, "JsonLdContext")?;
    Ok(())
}
