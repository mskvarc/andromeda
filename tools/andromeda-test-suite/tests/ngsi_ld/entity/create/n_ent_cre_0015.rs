//! An entity typed with the type keyword is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: a Create Entity whose type is the term "type", which the core @context maps to the @type keyword and so
/// names no entity type, is refused as `BadRequestData` and creates nothing
/// Clause: 5.6.1, 4.6.2, 5.5.4
///
/// Invalid type: type (the term the core @context maps to @type, so it names no entity type)
#[gtest]
#[tokio::test]
async fn n_ent_cre_0015_an_entity_typed_with_the_type_keyword_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"name": "https://test.andromeda/{namespace}/name"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = Entity::builder().id(building_id.clone()).entity_type("type").build()?;
    let answer = body_context.with_responses().entities().create(&building).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
