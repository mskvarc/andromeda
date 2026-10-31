//! Query subscriptions serves a limit of the cap and refuses one above it.

use andromeda_test_suite::{
    broker,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{headers::Accept, operation::QuerySubscriptions},
    model::{ErrorType, NgsiLdMediaType},
    subscription::EntitySubscription,
};

/// Pins: Query Subscriptions with limit 1000 answers 200, and with limit 1001 answers 403 `TooManyResults` whose
/// detail names the cap of 1000
/// Clause: 5.5.6, 5.5.9.1, 5.8.4, 6.3.10
/// Recorded: clause 5.5.6 leaves the `TooManyResults` threshold to the broker; Andromeda serves at most 1000
/// elements a page, and clause 5.5.9.1 makes the page size the requested limit, so a larger limit is refused rather
/// than served smaller
#[gtest]
#[tokio::test]
async fn a_lim_pgp_0004_query_subscriptions_serves_a_limit_of_the_cap_and_refuses_one_above_it() -> Result<()> {
    let broker = broker::front()?;
    // The subscriptions of the shared tenant are those of every file, some of which the typed model cannot read, and
    // the file pins the status alone.
    let page = QuerySubscriptions::builder().kind(EntitySubscription).limit(1000).build();
    let first = broker
        .send_unchecked(broker.unchecked(&page)?.append_header(ACCEPT, HeaderValue::from_static("application/json")))
        .await;
    expect_that!(&first, answered(StatusCode::OK));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(1001)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("at most 1000")
    );
    Ok(())
}
