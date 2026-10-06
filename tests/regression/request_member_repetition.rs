//! A request body whose object names one member twice is refused.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4 makes data that is not syntactically
//! correct according to the API data type definitions `BadRequestData`, and
//! IETF RFC 8259 section 4 asks the member names within an object to be unique
//! and gives a repeated name no defined meaning. The request path read a body in
//! place and kept the last of two attributes of one name, so the write that
//! passed for a client's mistake silently dropped the first of the two.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use crate::large_entity_budget::send_json;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

/// The problem type clause 5.5.4 gives data that is not syntactically correct.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Clause 5.5.4: an entity naming the attribute `speed` twice is
/// `BadRequestData` naming the repeated member and where it stands, and nothing
/// is created.
#[tokio::test]
async fn regression_request_member_repetition_a_duplicated_attribute_is_bad_request_data_naming_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:repeated-attribute");
    let body = format!(r#"{{"id":"{id}","type":"RepetitionProbe","speed":{{"type":"Property","value":10}},"speed":{{"type":"Property","value":20}}}}"#);

    let refused = send_json(client, Method::POST, "/ngsi-ld/v1/entities", body).await;
    let retrieved = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{id}"))
        .send()
        .await
        .expect("retrieve entity request");

    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    let problem = refused.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    let detail = problem["detail"].as_str().expect("a problem names its detail");
    assert!(detail.contains("/speed"), "the repeated member is named: {detail}");
    assert!(detail.contains("named twice"), "the repetition is named: {detail}");
    assert_eq!(retrieved.status, StatusCode::NOT_FOUND, "nothing was created: {:?}", retrieved.body);
    scope.finish().await.expect("cleanup failed");
}
