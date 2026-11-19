//! A linked cyclic iri mapping is refused and not cached.

use andromeda_test_suite::{broker, corrected_context::CorrectedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities under a Link naming a served @context whose document maps the term a to a compact IRI over b
/// and b to one over a answers 400 `BadRequestData` with a detail naming the URL and what is wrong and where, and
/// records no Cached entry for the URL: once the origin serves a valid @context there, the next request naming it
/// succeeds after a second download
/// Clause: 5.5.6, 5.13.1, 5.5.4, JSON-LD 1.1 API section 4.2.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9066_a_linked_cyclic_iri_mapping_is_refused_and_not_cached() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let linked = CorrectedContext::serving(r#"{"@context": {"a": "b:x", "b": "a:y"}}"#).await?;
    let answer = linked.query(&broker, &namespace).await?;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(&format!(
            "invalid JSON-LD context at {}: the @context cannot define the term a: cyclic IRI mapping",
            linked.url()
        ))
    );
    linked.verify_downloaded_again(&broker, &namespace).await
}
