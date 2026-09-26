//! The clause 5.5.4 `BadRequestData` answer a refused request must carry.

use andromeda_test_client::{problem_details::validate_problem_details, response::TestResponse};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

/// Assert that `response` is `400 Bad Request` carrying a `BadRequestData`
/// Problem Details body, naming `operation` in every failure message.
pub fn assert_bad_request_data(response: &TestResponse, operation: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{operation}: {:?}", response.body);
    let problem = response.json_value().expect("problem details must be JSON");
    let verdict = validate_problem_details(&problem, BAD_REQUEST_DATA);
    assert!(verdict.is_ok(), "{operation} must be refused as BadRequestData: {verdict:?} {problem}");
}
