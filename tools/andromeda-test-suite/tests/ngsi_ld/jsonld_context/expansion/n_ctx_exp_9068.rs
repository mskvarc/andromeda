//! A body context defining the empty term is refused with the term named in quotes.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity whose body @context defines the empty term answers 400 `BadRequestData` with a detail naming
/// the term as `""` and what the JSON-LD processor refused, and creates nothing
/// Clause: 5.5.4, 5.6.1.4, JSON-LD 1.1 API section 4.2.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9068_a_body_context_defining_the_empty_term_is_refused_with_the_term_named_in_quotes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"": {{"@id": "https://example.org/{namespace}/x/", "@type": "@id"}}}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let thing_id = namespace.urn("Thing", 1)?;
    let thing = Entity::builder().id(thing_id.clone()).entity_type(namespace.entity_type("Thing")?).build()?;
    let answer = body_context.with_responses().entities().create(&thing).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(r#"the @context cannot define the term "": invalid term definition"#)
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&thing_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
