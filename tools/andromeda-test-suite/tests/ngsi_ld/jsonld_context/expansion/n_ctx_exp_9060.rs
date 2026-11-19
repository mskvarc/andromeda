//! A context link that does not process is named in the detail.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities under a Link naming a served @context that defines a term as a number answers 400
/// `BadRequestData` with a detail naming the @context's URL, the term and what a term definition may be; under a
/// Link naming one that maps two terms to compact IRIs over each other it answers 400 `BadRequestData` with a
/// detail naming the @context's URL and the term, in neither case with the JSON-LD processor's own messages
/// Clause: 5.5.4, 5.5.6, 6.3.5, JSON-LD 1.1 section 9.15
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9060_a_context_link_that_does_not_process_is_named_in_the_detail() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let number = contexts
        .serve("jc/number.jsonld", r#"{"@context": {"speed": 42}}"#, Some("application/ld+json"))
        .await?;
    let cyclic = contexts
        .serve("jc/cyclic.jsonld", r#"{"@context": {"a": "b:x", "b": "a:y"}}"#, Some("application/ld+json"))
        .await?;
    let answer = broker
        .with_context(Context::Single(contexts.reference("jc/number.jsonld")?))?
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(&format!(
            "invalid JSON-LD context at {number}: the @context defines the term speed as a number, but a term definition is null, a string or an object"
        ))
    );
    let answer_2 = broker
        .with_context(Context::Single(contexts.reference("jc/cyclic.jsonld")?))?
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(&format!("the @context {cyclic} cannot define the term a: cyclic IRI mapping"))
    );
    Ok(())
}
