//! Expand values expands quoted words of a list of attributes and refuses bare ones.

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
    query::q::Query,
};
use std::result::Result as StdResult;

/// Pins: expandValues=status,mode expands the quoted words of status!="idle";mode=="night" through the request
/// @context and refuses the same words written bare with 400 `BadRequestData`, and `expandValues=status` leaves a
/// quoted string for the other attribute unexpanded
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 5.5.7
/// Recorded: 4.9 derives an unquoted value only from Number, `dateTime`, date, time, true, false and URI, while its
/// EXAMPLE 12 writes ?q=gender==Male&expandValues=gender; the normative grammar is followed, so a term to expand is
/// written as a `quotedStr`
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4012_expand_values_expands_quoted_words_of_a_list_of_attributes_and_refuses_bare_ones() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "mode": {{"@id": "https://test.andromeda/{namespace}/mode", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active", "idle": "https://test.andromeda/{namespace}/state/idle", "night": "https://test.andromeda/{namespace}/mode/night", "day": "https://test.andromeda/{namespace}/mode/day"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let states = [("active", "night"), ("idle", "night"), ("active", "day")];
    let mut devices = Vec::new();
    for (n, (state, mode)) in states.into_iter().enumerate() {
        devices.push(
            Entity::builder()
                .id(namespace.urn("Device", n + 1)?)
                .entity_type(device_type.clone())
                .property("status", format!("https://test.andromeda/{namespace}/state/{state}"))
                .property("mode", format!("https://test.andromeda/{namespace}/mode/{mode}"))
                .build()?,
        );
    }
    linked.entities().batch_create(&devices).send().await?.done()?;
    let json = linked.with_accept(Accept::new(NgsiLdMediaType::Json));
    let attributes = |names: &[&str]| names.iter().map(|name| name.parse::<AttributeName>()).collect::<StdResult<Vec<_>, _>>();
    let both = json
        .entities()
        .query()
        .entity_type(&device_type)
        .q(r#"status!="idle";mode=="night""#.parse::<Query>()?)
        .expand_values(attributes(&["status", "mode"])?)
        .send()
        .await?;
    expect_that!(both.items(), len(eq(1)));
    expect_that!(both.items(), contains(has_id(&device_id)));
    let status_only = json
        .entities()
        .query()
        .entity_type(&device_type)
        .q(r#"status=="active";mode=="night""#.parse::<Query>()?)
        .expand_values(attributes(&["status"])?)
        .send()
        .await?;
    expect_that!(status_only.items(), is_empty());
    // The query language refuses bare words, which the typed query never writes, so this read goes out as written.
    let type_text = device_type.to_string();
    let link = format!(r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#, hosted.url);
    let read = target(
        "entities",
        &[("type", type_text.as_str()), ("q", "status!=idle;mode==night"), ("expandValues", "status,mode")],
    );
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &read,
            &[("Accept", "application/json"), ("Link", link.as_str())],
            None,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
