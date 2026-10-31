//! A text past the request reader is invalid request.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose p value nests 126 objects, past the 127 containers the request reader takes, answers
/// 400 `InvalidRequest` and stores nothing
/// Clause: 5.6.1.4, 5.5.4, 6.3.2
#[gtest]
#[tokio::test]
async fn a_lim_dep_9014_a_text_past_the_request_reader_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe = Entity::builder().id(nesting_probe_id.clone()).entity_type(nesting_probe_type).build()?;
    // The model refuses it (recursion limit exceeded at line 1 column 857), so `p` is written as text.
    let p: RawJson = r#"{"type": "Property", "value": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": {"k": "leaf"}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&nesting_probe, Lossless::Normalized).set_member("p", &p);
    let request = broker.unchecked(&CreateEntity::builder().entity(&nesting_probe).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
