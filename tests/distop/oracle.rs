//! Shared assertion oracles for the distributed-operations suite, so a test's
//! expectations read uniformly: a status code, a
//! Problem Details type, a warning code, an echoed entity-map, a `Via` chain,
//! or a forwarded-request shape.

use andromeda_test_client::{entity::results_count, problem_details::validate_problem_details, response::TestResponse};
use andromeda_test_mocks::callback_server::CapturedRequest;
use iri_rs::Iri;
use serde_json::Value;

/// Assert the response carries exactly the expected HTTP status. On mismatch the
/// panic message includes the body, since a distributed failure is usually only
/// legible through the Problem Details or `BatchOperationResult` it returned.
pub fn assert_status(response: &TestResponse, expected: u16) {
    let actual = response.status.as_u16();
    assert!(
        actual == expected,
        "expected status {expected}, got {actual}; body: {}",
        String::from_utf8_lossy(&response.body)
    );
}

/// Assert the response is a Problem Details document of the expected NGSI-LD
/// error type with a non-empty `title` (clause 5.5.2 / 6.3.16).
pub fn assert_problem_details(response: &TestResponse, expected_type: &Iri<&str>) {
    let body = json_body(response);
    assert!(
        validate_problem_details(&body, expected_type).is_ok(),
        "problem details mismatch for {expected_type}; body: {body}"
    );
}

/// Decode the response body as JSON, failing with the raw body on error.
pub fn json_body(response: &TestResponse) -> Value {
    response
        .json_value()
        .map_err(|error| format!("{error}; raw: {}", String::from_utf8_lossy(&response.body)))
        .expect("the response body is JSON")
}

/// The `Location` header value, required to be present (used after a 201).
pub fn location(response: &TestResponse) -> String {
    response
        .location()
        .expect("Location header is valid text")
        .expect("response carries a Location header")
        .to_owned()
}

/// The `NGSILD-Results-Count` header, if present.
pub fn results_count_header(response: &TestResponse) -> Option<usize> {
    results_count(&response.headers)
}

/// The `NGSILD-EntityMap` header value, if present (clause 6.3.15).
pub fn entity_map_header(response: &TestResponse) -> Option<String> {
    header_text(response, "ngsild-entitymap")
}

/// The leading integer codes of every `NGSILD-Warning` header on the response.
///
/// NGSI-LD reuses the HTTP `Warning` grammar `<code> <agent> "<text>"`
/// (clause 6.3.11), so the code is the first whitespace-delimited token.
pub fn warning_codes(response: &TestResponse) -> Vec<u16> {
    response
        .headers
        .get_all("ngsild-warning")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(leading_code)
        .collect()
}

/// Whether any `NGSILD-Warning` on the response carries the given code.
pub fn has_warning(response: &TestResponse, code: u16) -> bool {
    warning_codes(response).contains(&code)
}

/// The `contextSourceAlias` pseudonyms listed in a forwarded request's `Via`
/// header, in order, used to prove cascade limiting and loop prevention
/// (clause 4.3.6.4 / 6.3.18). The `Via` grammar is a comma-separated list of
/// `<protocol> <pseudonym>` entries; the pseudonym is the last token of each.
pub fn via_pseudonyms(request: &CapturedRequest) -> Vec<String> {
    request
        .headers
        .get_all("via")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|entry| entry.split_whitespace().last().map(str::to_owned))
        .collect()
}

/// The value of one header on a forwarded request, as text.
pub fn forwarded_header(request: &CapturedRequest, name: &str) -> Option<String> {
    request.headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
}

/// Read a response header as text.
fn header_text(response: &TestResponse, name: &str) -> Option<String> {
    response.headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
}

/// Parse the leading integer of a `Warning`-style header value.
fn leading_code(value: &str) -> Option<u16> {
    value.split_whitespace().next()?.parse().ok()
}
