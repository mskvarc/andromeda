//! A post query expand values refuses a bare word and expands the quoted one.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST with `expandValues` status in a body that carries the context refuses q
/// status==active with 400 `BadRequestData`, and expands q status=="active" through that context, so it matches the
/// IRI the stored value holds
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1, 5.5.7
/// Recorded: 4.9 derives an unquoted value only from Number, `dateTime`, date, time, true, false and URI, while its
/// EXAMPLE 12 writes ?q=gender==Male&expandValues=gender; the normative grammar is followed, so a term to expand is
/// written as a `quotedStr`
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4011_a_post_query_expand_values_refuses_a_bare_word_and_expands_the_quoted_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active", "idle": "https://test.andromeda/{namespace}/state/idle"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let devices = [
        Entity::builder()
            .id(device_id.clone())
            .entity_type(device_type.clone())
            .property("status", format!("https://test.andromeda/{namespace}/state/active"))
            .build()?,
        Entity::builder()
            .id(namespace.urn("Device", 2)?)
            .entity_type(device_type.clone())
            .property("status", format!("https://test.andromeda/{namespace}/state/idle"))
            .build()?,
    ];
    linked.entities().batch_create(&devices).send().await?.done()?;
    // The query language refuses a bare word, which the typed query never writes, so this body goes out as written.
    let bare = format!(
        r#"{{"type": "Query", "entities": [{{"type": "{device_type}"}}], "q": "status==active", "expandValues": "status", "@context": "{}"}}"#,
        hosted.url
    );
    let answer = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "entityOperations/query",
            &[("Content-Type", "application/ld+json"), ("Accept", "application/json")],
            Some(bare.as_str()),
        )?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&device_type)
        .q(q::attr("status").eq("active"))
        .expand_values(vec!["status".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&device_id)));
    Ok(())
}
