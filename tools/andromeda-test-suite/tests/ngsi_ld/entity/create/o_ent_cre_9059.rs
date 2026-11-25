//! A prefixed `objectType` holding a hyphen after the colon is bad request data.

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

/// Pins: Create Entity whose @context defines the prefix ex and whose Relationship owner has the objectType
/// ex:bad-type answers 400 `BadRequestData` and creates nothing
/// Clause: 4.5.3.2, 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; an
/// objectType names entity types, so Andromeda holds it to the grammar an entity type is held to
#[gtest]
#[tokio::test]
async fn o_ent_cre_9059_a_prefixed_object_type_holding_a_hyphen_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "prefixed-object-type")?;
    // The objectType is the point of the test and the model refuses it, so the body is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            r#"{{"id": "{vehicle_id}", "type": "Vehicle_{namespace}", "owner": {{"type": "Relationship", "object": "urn:ngsi-ld:Person:o", "objectType": "ex:bad-type"}}, "@context": {{"ex": "http://example.org/"}}}}"#
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
