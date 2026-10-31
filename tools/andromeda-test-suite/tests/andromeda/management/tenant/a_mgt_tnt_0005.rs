//! Post on a tenant is method not allowed.

use andromeda_test_suite::{broker, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_LENGTH};

/// Pins: POST `/tenants/evictOrphans` reaches the tenant resource of that name, which serves only GET and PATCH,
/// and is answered 405: no tenant operation is served under POST
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0005_post_on_a_tenant_is_method_not_allowed() -> Result<()> {
    let broker = broker::front()?;
    // The request declares an empty body, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "tenants/evictOrphans")?
        .append_header(CONTENT_LENGTH, HeaderValue::from_static("0"))
        .body(Bytes::from(""));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::METHOD_NOT_ALLOWED));
    Ok(())
}
