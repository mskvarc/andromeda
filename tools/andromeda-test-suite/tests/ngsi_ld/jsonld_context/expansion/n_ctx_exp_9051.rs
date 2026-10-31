//! An unquoted link type resolves the context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, Lossless, NgsiLdMediaType, OneOrMany, Output},
};
use std::str;

/// Pins: a context Link whose type parameter is a bare media type resolves the context as a quoted one does, and a
/// type that is empty or not a media type is refused as `BadRequestData`, quoted or not
/// Clause: 6.3.5, 5.5.4; IETF RFC 8288 Appendix B.3
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9051_an_unquoted_link_type_resolves_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    let link = |media_type: &str| HeaderValue::from_str(&format!(r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type={media_type}"#, ctx.url));
    // The type parameter is written without quotes, empty or not a media type, which no typed Link writes.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(LINK, link("application/ld+json")?)
        .document(&UncheckedDocument::from_entity(&building, Lossless::Normalized));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let read = |media_type: &str| -> Result<_> {
        Ok(request_to(&broker, Method::GET, &format!("entities/{building_id}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"))
            .append_header(LINK, link(media_type)?))
    };
    let answer_2 = broker.send_unchecked(read("application/ld+json")?).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let entity = Entity::<Output>::parse(str::from_utf8(answer_2?.body())?, Lossless::Normalized)?;
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(&entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let quoted = retrieved.body();
    expect_that!(quoted.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(quoted, has_attribute("name", any_attribute().value("Eiffel Tower")));
    for refused_type in ["", "not a type", "\"not a type\""] {
        let answer_3 = broker.send_unchecked(read(refused_type)?).await;
        expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
