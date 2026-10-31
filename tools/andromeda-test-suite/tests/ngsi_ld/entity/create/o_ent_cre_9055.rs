//! A prefixed entity type holding a hyphen after the colon is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity whose @context defines the prefix ex and whose type is ex:bad-type answers 400 `BadRequestData` and creates nothing
/// Clause: 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_ent_cre_9055_a_prefixed_entity_type_holding_a_hyphen_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "prefixed-type")?;
    // The name is the point of the test and the model refuses it, so the body is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            r#"{{"id": "urn:ngsi-ld:Vehicle:{namespace}-prefixed-type", "type": "ex:bad-type-{namespace}", "speed": {{"type": "Property", "value": 1}}, "@context": {{"ex": "http://example.org/"}}}}"#
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
