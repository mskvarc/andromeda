//! Every payload reader refuses a body over the limit and closes.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::spaces,
    namespace::Namespace,
    problem::{problem, problem_status, request_entity_too_large},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONNECTION};

/// Pins: Create Entity, Batch Entity Upsert, Add @context, Update `EntityMap` and a tenant update, each sent one
/// octet more than the request body limit while the client keeps writing, answer 413 `RequestEntityTooLarge` naming
/// the limit in bytes, with Connection close
/// Clause: 5.5.3, 6.3.2
/// Recorded: Andromeda configuration, not v1.9.1: 6.3.2 answers 413 to "too many bytes" without fixing a size; the
/// stack runs the default `http.max_request_body` of 64 `MiB`, and a refusal is answered before the upload is read
///
/// An empty Expect header keeps curl from waiting for 100 Continue, so the refusal arrives while the body is still
/// being written.
#[gtest]
#[tokio::test]
async fn a_lim_pay_9010_every_payload_reader_refuses_a_body_over_the_limit_and_closes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let limit = SuiteConfig::get()?.budget(Budget::MaxRequestBody)?;
    // The body is whitespace one octet over the limit, a text no operation writes and the size is the only reason to
    // refuse it.
    let body = spaces(limit + 1)?;
    for (method, target) in [
        (Method::POST, "entities".to_owned()),
        (Method::POST, "entityOperations/upsert".to_owned()),
        (Method::POST, "jsonldContexts".to_owned()),
        (Method::PATCH, format!("entityMaps/urn:ngsi-ld:EntityMap:{namespace}-too-large")),
        (Method::PATCH, format!("tenants/{namespace}")),
    ] {
        let refused = broker.send_unchecked(json_request_to(&broker, method, &target, &body)?).await;
        expect_that!(
            &refused,
            problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
                .with_detail_containing(&format!("{limit} bytes"))
                .with_header(CONNECTION, "close")
        );
        expect_that!(problem_status(&refused), some(eq(413)));
    }
    Ok(())
}
