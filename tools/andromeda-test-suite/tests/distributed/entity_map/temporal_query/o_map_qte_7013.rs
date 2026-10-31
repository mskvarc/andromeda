//! Expand values expands the quoted word of q through the request context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `expandValues` status expands the quoted
/// string of q status=="active" through the request @context, where status is coerced as @vocab, so the map holds
/// the Entity whose observations store that IRI and not the one holding another state
/// Clause: 5.14.5.4, 4.9, 6.4.3.2, 5.5.7
/// Recorded: Table 6.4.3.2-1 says the values of the attributes `expandValues` names "should be expanded against the
/// supplied @context using JSON-LD type coercion prior to executing the query", which does not depend on how the
/// value is written
#[gtest]
#[tokio::test]
async fn o_map_qte_7013_expand_values_expands_the_quoted_word_of_q_through_the_request_context() -> Result<()> {
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
    let device_evolution = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property(
            "status",
            Property::builder(format!("https://test.andromeda/{namespace}/state/active")).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    body_context.temporal().create_or_update(&device_evolution).send().await?.created()?;
    let device_2_evolution = TemporalEntity::builder()
        .id(device_2_id)
        .entity_type(device_type.clone())
        .property(
            "status",
            Property::builder(format!("https://test.andromeda/{namespace}/state/idle")).observed_at("2021-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    body_context.temporal().create_or_update(&device_2_evolution).send().await?.created()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let created = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
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
