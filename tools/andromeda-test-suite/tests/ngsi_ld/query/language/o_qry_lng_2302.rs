//! Query entities by post with an empty language is refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem_among, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities by POST with an empty lang parameter is refused with 400, `BadRequestData` or
/// `InvalidRequest`
/// Clause: 4.15, 5.7.2.3, 6.23.3.1
/// Recorded: clause 4.15 defines lang as a langtag, which an empty value is not, but no clause names the error for
/// it; Table 5.5.2-1 makes `BadRequestData` and `InvalidRequest` both plausible, so both are accepted
#[gtest]
#[tokio::test]
async fn o_qry_lng_2302_query_entities_by_post_with_an_empty_language_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::POST,
        "entityOperations/query?lang=",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(format!(
        "{{\n    \"type\": \"Query\",\n    \"entities\": [{{ \"type\": \"Building_{namespace}\" }}]\n}}"
    )));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    Ok(())
}
