//! A post query naming two context links is refused.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities by POST as application/json carrying two @context Link lines is refused as `BadRequestData`
/// Clause: 6.3.5, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0046_a_post_query_naming_two_context_links_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    // The client writes one Link field, so the request naming several is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/query")?.append_header(HeaderName::from_static("link"), HeaderValue::try_from(format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""))?).append_header(HeaderName::from_static("link"), HeaderValue::from_static("<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Vehicle_{namespace}\"\n        }}\n    ]\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
