//! Generated filters select alike in a query and a subscription.

use andromeda_test_suite::{broker, namespace::Namespace, q_generation::GeneratedCase, q_parity::expect_generated_case_to_agree};
use futures::future::join_all;
use googletest::prelude::*;

/// The seeds the cases are drawn from, each drawing the same case on every run.
const SEEDS: u64 = 64;

/// The Entities each seed draws.
const ENTITIES: usize = 20;

/// The filters each seed draws.
const FILTERS: usize = 30;

/// Creates the Entities `seed` draws under a namespace of its own and expects Query Entities and a Subscription with
/// each filter it draws to agree on the Entities selected.
async fn expect_drawn_case_to_agree(seed: u64) -> Result<()> {
    let case = GeneratedCase::drawn(seed, ENTITIES, FILTERS)?;
    expect_generated_case_to_agree(&broker::front()?, &Namespace::fresh(), seed, &case).await
}

/// Pins: for 64 fixed seeds, each drawing 20 Entities over eight Attribute names of every attribute family and 30
/// filters over the same names, 1920 filters in all, Query Entities and a Subscription with each filter and the
/// `jsonKeys` and `expandValues` drawn with it select the same Entities; clause 4.9 gives a Query Term one meaning
/// wherever it is evaluated
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9216_generated_filters_select_alike_in_a_query_and_a_subscription() -> Result<()> {
    for outcome in join_all((0..SEEDS).map(expect_drawn_case_to_agree)).await {
        outcome?;
    }
    Ok(())
}
