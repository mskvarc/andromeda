//! Query registrations serves a limit of the cap and refuses one above it.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Context Source Registrations with limit 1000 answers 200, and with limit 1001 answers 403
/// `TooManyResults` whose detail names the cap of 1000
/// Clause: 5.5.6, 5.5.9.1, 5.10.2, 6.3.10
/// Recorded: clause 5.5.6 leaves the `TooManyResults` threshold to the broker; Andromeda serves at most 1000
/// elements a page, and clause 5.5.9.1 makes the page size the requested limit, so a larger limit is refused rather
/// than served smaller
#[gtest]
#[tokio::test]
async fn a_lim_pgp_0003_query_registrations_serves_a_limit_of_the_cap_and_refuses_one_above_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Vehicle")?)
        .limit(1000)
        .send()
        .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Vehicle")?)
        .limit(1001)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("at most 1000")
    );
    Ok(())
}
