//! The configured extras are answered verbatim.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    raw_answer::send_raw,
    source_extensions::extensions,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information as application/ld+json answers the `contextSourceExtras` the
/// deployment configures, deployment multi beside the advertised extensions, as raw JSON
/// Clause: 5.15.1.4, 5.2.40
#[gtest]
#[tokio::test]
async fn a_inf_ret_6003_the_configured_extras_are_answered_verbatim() -> Result<()> {
    let broker = broker::front()?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).source_identity().send().await?;
    expect_that!(extensions(&identity)?.first().map(String::as_str), some(eq("cursorPagination")));
    // The extras are raw JSON in the model, so the members the test pins are read from the answer as received.
    let read = request_to(&broker, Method::GET, "info/sourceIdentity")?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let response = send_raw(read).await?;
    expect_that!(response.body(), json_equals("$.type", "ContextSourceIdentity"));
    expect_that!(response.body(), json_equals("$.contextSourceExtras.deployment", "multi"));
    expect_that!(response.body(), json_equals("$.contextSourceExtras.extensions[0]", "cursorPagination"));
    expect_that!(response.body(), json_absent("$.contextSourceExtras['@context']"));
    Ok(())
}
