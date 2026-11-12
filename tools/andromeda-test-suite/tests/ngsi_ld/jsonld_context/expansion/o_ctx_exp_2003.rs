//! A relative context reference in a temporal body resolves as create does.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::answered, relative_context::with_context_text};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{AddTemporalAttributes, CreateOrUpdateTemporalEvolution, NormalizedTemporal, QueryMethod, QueryTemporalEvolution},
    },
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{builders::SelectorTypeBuilder, temporal_query},
};
use serde_json::{Value, json};

/// Pins: the Location of a stored @context, a reference relative to the broker's origin, resolves as the body
/// @context of Create Temporal Evolution, Add Attributes to Temporal Evolution and Query Temporal Evolution of
/// Entities by POST, as it does for Create Entity
/// Clause: 5.13.2, 6.3.5, 5.6.11, 5.6.12, 5.7.4
/// Recorded: the specification does not say how a relative context reference resolves; the defect is that one
/// broker operation resolves its own Location and another refuses it
/// Vocabulary: none (the test is about @context resolution itself)
#[gtest]
#[tokio::test]
async fn o_ctx_exp_2003_a_relative_context_reference_in_a_temporal_body_resolves_as_create_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "speed": "https://test.andromeda/{namespace}/speed"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    expect_that!(ctx.location.as_str(), starts_with("/ngsi-ld/v1/jsonldContexts/"));
    let body_context = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let reference = json!(ctx.location);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    // The client writes no relative reference into a body, so each body's @context is replaced by the Location.
    let request = with_context_text(
        body_context.unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?,
        &reference,
    )?;
    let created = body_context.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(41).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?
        .attributes;
    let request_2 = with_context_text(
        body_context.unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&instances).build())?,
        &reference,
    )?;
    let added = body_context.send_unchecked(request_2).await;
    expect_that!(&added, answered(StatusCode::NO_CONTENT));
    let temporal_q = temporal_query::after("2019-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&temporal_q)
        .entity_type(&selector)
        .method(QueryMethod::Post)
        .format(NormalizedTemporal)
        .build();
    let request_3 = with_context_text(body_context.with_accept(Accept::new(NgsiLdMediaType::Json)).unchecked(&query)?, &reference)?;
    let queried = body_context.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&queried, answered(StatusCode::OK));
    let page: Value = serde_json::from_slice(queried?.body())?;
    expect_that!(page.as_array().map(Vec::len), some(eq(1)));
    expect_that!(page.pointer("/0/id"), some(eq(&json!(vehicle_id.to_string()))));
    Ok(())
}
