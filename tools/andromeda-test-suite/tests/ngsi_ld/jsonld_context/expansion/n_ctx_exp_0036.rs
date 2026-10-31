//! A relative context link is resolved against the request uri.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    wire::request_to,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType},
};

/// Pins: A create under a Link naming a stored @context by the root-relative path Add @context answered expands its
/// terms through that @context
/// Clause: 6.3.5, 6.29.3.1, RFC 8288 s3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0036_a_relative_context_link_is_resolved_against_the_request_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"speed": "https://test.andromeda/{namespace}/relative/speed"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 3)
        .build()?;
    // The Link names the stored context by its root-relative path, which no typed context entry spells.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(
                r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
                ctx.url.path()
            ))?,
        )
        .document(&UncheckedDocument::from_entity(&vehicle, Lossless::Normalized));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/relative/speed"), any_attribute().value(3))
    );
    Ok(())
}
