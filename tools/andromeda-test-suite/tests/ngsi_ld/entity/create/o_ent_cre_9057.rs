//! A sub-attribute name the request's @context expands to no IRI is bad request data.

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
use serde_json::json;

/// Names whose expansion under `{"ex": "http://example.org/"}` is no absolute IRI: an IRI admits no `{`, `"` or `%`
/// that two hexadecimal digits do not follow (IETF RFC 3987), and `urn` and `https` are no terms of the context.
const UNEXPANDABLE_NAMES: [&str; 6] = ["ex:a{b", "ex:{", "ex:%", "ex:\"", "urn:x:{", "https://example.org/{"];

/// Pins: Create Entity whose @context defines the prefix ex and whose Property speed holds a sub-attribute named
/// `ex:a{b`, `ex:{`, `ex:%`, `ex:"`, `urn:x:{` or `https://example.org/{` answers 400 `BadRequestData` for each and
/// creates nothing
/// Clause: 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_ent_cre_9057_a_sub_attribute_name_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    for (position, name) in UNEXPANDABLE_NAMES.into_iter().enumerate() {
        let vehicle_id = namespace.urn("Vehicle", position)?;
        // The name is the point of the test and the model refuses it, so the body is sent as written.
        let body = json!({
            "id": vehicle_id.to_string(),
            "type": format!("Vehicle_{namespace}"),
            "speed": {"type": "Property", "value": 1, name: {"type": "Property", "value": 2}},
            "@context": {"ex": "http://example.org/"}
        });
        let request = request_to(&broker, Method::POST, "entities")?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
            .body(Bytes::from(serde_json::to_vec(&body)?));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{name}");
        let answer_2 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await;
        expect_that!(&answer_2, answered(StatusCode::NOT_FOUND), "{name}");
    }
    Ok(())
}
