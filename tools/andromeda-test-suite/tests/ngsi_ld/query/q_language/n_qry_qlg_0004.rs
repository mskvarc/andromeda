//! Expand values refuses a bare word and expands the quoted one.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
    query::q,
};

/// Pins: with `expandValues=status`, the q value active written bare is refused with 400 `BadRequestData`, and
/// written quoted it expands through the request @context, where status is coerced as @vocab, and matches the IRI
/// the stored value holds
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 5.5.7
/// Recorded: 4.9 derives an unquoted value only from Number, `dateTime`, date, time, true, false and URI, while its
/// EXAMPLE 12 writes ?q=gender==Male&expandValues=gender; the normative grammar is followed, so a term to expand is
/// written as a `quotedStr`
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0004_expand_values_refuses_a_bare_word_and_expands_the_quoted_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("status", format!("https://test.andromeda/{namespace}/state/active"))
        .build()?;
    linked.entities().create(&device).send().await?.done()?;
    // The query language refuses a bare word, which the typed query never writes, so this read goes out as written.
    let id_text = device_id.to_string();
    let link = format!(r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#, hosted.url);
    let read = target("entities", &[("id", id_text.as_str()), ("q", "status==active"), ("expandValues", "status")]);
    let refused = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Link", link.as_str())], None)?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![device_id.clone()])
        .q(q::attr("status").eq("active"))
        .expand_values(vec!["status".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&device_id)));
    Ok(())
}
