//! A bare word without expand values is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: a q value written as a bare word (status==active) for an attribute `expandValues` does not name is outside
/// the 4.9 grammar and refused with 400 `BadRequestData`
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0006_a_bare_word_without_expand_values_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device = Entity::builder()
        .id(device_id)
        .entity_type(device_type)
        .property("status", format!("https://test.andromeda/{namespace}/state/active"))
        .build()?;
    body_context.entities().create(&device).send().await?.done()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &linked.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3ADevice%3A{namespace}-1&q=status%3D%3Dactive"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = linked.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
