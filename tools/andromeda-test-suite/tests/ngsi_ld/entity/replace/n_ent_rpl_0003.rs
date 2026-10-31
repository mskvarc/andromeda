//! An id that is not a uri is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Replace Entity addressed to an id that is not a URI is refused with 400 `BadRequestData`
/// Clause: 5.6.18.4, 6.5.3.3
#[gtest]
#[tokio::test]
async fn n_ent_rpl_0003_an_id_that_is_not_a_uri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The model refuses this entity, so the replacement is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/invalid-{namespace}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"invalid-{namespace}\",\n    \"type\": \"Building_{namespace}\",\n    \"name\": {{\n        \"type\": \"Property\",\n        \"value\": \"Pisa Tower\"\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
