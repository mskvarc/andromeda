//! List contexts serves a limit of the cap and refuses one above it.

use andromeda_test_suite::{
    broker,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: List @contexts with limit 1000 answers 200, and with limit 1001 answers 403 `TooManyResults` whose detail
/// names the cap of 1000
/// Clause: 5.5.6, 5.5.9.1, 5.13.3, 6.3.10
/// Recorded: clause 5.5.6 leaves the `TooManyResults` threshold to the broker; Andromeda serves at most 1000
/// elements a page, and clause 5.5.9.1 makes the page size the requested limit, so a larger limit is refused rather
/// than served smaller
#[gtest]
#[tokio::test]
async fn a_lim_pgp_0006_list_contexts_serves_a_limit_of_the_cap_and_refuses_one_above_it() -> Result<()> {
    let broker = broker::front()?;
    // The typed List @contexts takes no limit, so the request is sent as written.
    let request = request_to(&broker, Method::GET, "jsonldContexts?limit=1000")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    // The typed List @contexts takes no limit, so the request is sent as written.
    let request_2 = request_to(&broker, Method::GET, "jsonldContexts?limit=1001")?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("at most 1000")
    );
    Ok(())
}
