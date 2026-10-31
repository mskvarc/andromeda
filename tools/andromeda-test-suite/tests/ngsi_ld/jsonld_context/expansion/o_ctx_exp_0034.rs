//! A malformed or non ascii link is refused with or without a body.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Retrieve Entity and Query Entities by POST as application/json carrying a Link that is not a link, or one
/// that is not visible ASCII, are refused as `BadRequestData`
/// Clause: 6.3.5, 5.5.4, RFC 8288 s3, RFC 9110 s5.5
/// Recorded: clause 6.3.5 does not say how a Link the broker cannot read is answered; Andromeda refuses it as
/// `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0034_a_malformed_or_non_ascii_link_is_refused_with_or_without_a_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}"}}]}}"#);
    let non_ascii = format!(r#"<https://test.andromeda/{namespace}/é.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#);
    for link in ["this is not a link", non_ascii.as_str()] {
        // The typed client writes only a well-formed Link, so the request is sent as written.
        let retrieve = request_to(&broker, Method::GET, &format!("entities/{vehicle_id}"))?.append_header(LINK, HeaderValue::from_str(link)?);
        let answer = broker.send_unchecked(retrieve).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        // The typed client writes only a well-formed Link, so the request is sent as written.
        let post = request_to(&broker, Method::POST, "entityOperations/query")?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .append_header(LINK, HeaderValue::from_str(link)?)
            .body(Bytes::copy_from_slice(query.as_bytes()));
        let answer_2 = broker.send_unchecked(post).await;
        expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
