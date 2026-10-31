//! An empty or repeated options keyword is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, raw_call::target, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: on the tenant collection and on one tenant, an options value holding an empty keyword (sysAttrs,,usage) or a
/// repeated keyword (usage,usage) answers 400 `InvalidRequest`, as every other options parameter does
/// Clause: 6.3.20
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0009_an_empty_or_repeated_options_keyword_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    // The Tenant resource and its `options` are an Andromeda extension no NGSI-LD operation covers, so the requests are
    // sent as written.
    for path in ["tenants".to_owned(), format!("tenants/{namespace}")] {
        for options in ["sysAttrs,,usage", "usage,usage"] {
            let request =
                request_to(&front, Method::GET, &target(&path, &[("options", options)]))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
            let answer = front.send_unchecked(request).await;
            expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
        }
    }
    Ok(())
}
