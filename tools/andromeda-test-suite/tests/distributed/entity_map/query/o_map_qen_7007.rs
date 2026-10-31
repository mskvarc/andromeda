//! Expand values expands the quoted word of q through the request context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    query::q,
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Entities with `expandValues` status expands the quoted string of q
/// status=="active" through the request @context, where status is coerced as @vocab, so the map holds the Entity
/// whose stored value is that IRI and not the one holding another state
/// Clause: 5.14.4.4, 4.9, 6.4.3.2, 5.5.7
/// Recorded: Table 6.4.3.2-1 says the values of the attributes `expandValues` names "should be expanded against the
/// supplied @context using JSON-LD type coercion prior to executing the query", which does not depend on how the
/// value is written
#[gtest]
#[tokio::test]
async fn o_map_qen_7007_expand_values_expands_the_quoted_word_of_q_through_the_request_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"status": {{"@id": "https://test.andromeda/{namespace}/status", "@type": "@vocab"}}, "active": "https://test.andromeda/{namespace}/state/active"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let entities = [
        Entity::builder()
            .id(device_id.clone())
            .entity_type(device_type.clone())
            .property("status", format!("https://test.andromeda/{namespace}/state/active"))
            .build()?,
        Entity::builder()
            .id(device_2_id)
            .entity_type(device_type.clone())
            .property("status", format!("https://test.andromeda/{namespace}/state/idle"))
            .build()?,
    ];
    body_context.entities().batch_create(&entities).send().await?.done()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let created = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&device_type)
        .q(q::attr("status").eq("active"))
        .expand_values(vec!["status".parse()?])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&device_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
