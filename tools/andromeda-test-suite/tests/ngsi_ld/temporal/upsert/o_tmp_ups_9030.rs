//! A prefixed attribute name breaking the grammar after the colon is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: a Temporal Evolution whose attribute is named ex:bad-name under an @context defining the prefix ex is refused
/// with 400 `BadRequestData` and the entity is not created
/// Clause: 4.6.2, 5.6.11.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9030_a_prefixed_attribute_name_breaking_the_grammar_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    // The attribute name is the point of the test and the model refuses it, so the body is sent as written.
    let request = request_to(&broker, Method::POST, "temporal/entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            r#"{{"id": "urn:ngsi-ld:Vehicle:{namespace}-1", "type": "Vehicle_{namespace}", "ex:bad-name": [{{"type": "Property", "value": 1, "observedAt": "2020-08-01T12:03:00Z"}}], "@context": {{"ex": "http://example.org/"}}}}"#
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
