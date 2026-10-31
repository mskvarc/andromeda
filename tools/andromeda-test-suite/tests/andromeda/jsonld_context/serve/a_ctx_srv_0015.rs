//! Serve an implicitly created context carries the same validators.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    hosted_context::HostedContext,
    minted_wrapper::minted_at,
    namespace::Namespace,
    problem::answered,
    served_context::{ServedContext, single_header, varies_by_tenant_and_encoding},
};
use googletest::prelude::*;
use http::{
    StatusCode,
    header::{CACHE_CONTROL, ETAG},
};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Serve @context of an `ImplicitlyCreated` document answers 200 with an `ETag`, a Vary naming NGSILD-Tenant
/// and the Cache-Control of a Hosted document
/// Clause: 5.13.4.4, 5.13.1, RFC 7232 s2.3
/// Tenant: private (the subscription's wrapper is identified by its content, which every run repeats; clause 4.14
/// keeps a tenant's stored @contexts its own)
#[gtest]
#[tokio::test]
async fn a_ctx_srv_0015_serve_an_implicitly_created_context_carries_the_same_validators() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let hosted = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let served = ServedContext::of(&broker, &hosted)?;
    let answer = served.get(&[]).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let cache = single_header(answer.as_ref().or_fail()?, &CACHE_CONTROL)?;
    expect_that!(cache, matches_regex(r"^max-age=[1-9][0-9]*$"));
    let subscription_id = namespace.subscription_id(1)?;
    let linked = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"T1_{namespace}": "https://test.andromeda/{namespace}/T1"}}"#).parse::<InlineContext>()?),
            ContextEntry::Inline(format!(r#"{{"speed": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([namespace.suffixed("T1")])
        .notification(NotificationParams::builder(Endpoint::builder(config.unreachable_origin().join("notify")?.as_str()).build()?).build()?)
        .build()?;
    linked.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let wrapper = minted_at(retrieved.jsonld_context().or_fail()?)?;
    let wrapper_served = ServedContext::at(&broker, &wrapper.id.to_string());
    let wrapper_answer = wrapper_served.get(&[]).await?;
    expect_that!(&wrapper_answer, answered(StatusCode::OK).with_header(CACHE_CONTROL, &cache));
    let wrapper_response = wrapper_answer.as_ref().or_fail()?;
    expect_that!(wrapper_response.headers(), varies_by_tenant_and_encoding());
    expect_that!(single_header(wrapper_response, &ETAG)?, matches_regex(r#"^"[^"]+"$"#));
    Ok(())
}
