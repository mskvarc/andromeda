//! Generated filters select alike in a query and a subscription.

use andromeda_test_suite::{
    broker,
    drawn_selection::{SelectionBounds, Selections},
    namespace::Namespace,
    q_generation::GeneratedCase,
    q_parity::expect_generated_case_to_agree,
};
use futures::future::join_all;
use googletest::prelude::*;

/// The seeds the cases are drawn from, each drawing the same case on every run.
const SEEDS: u64 = 64;

/// The Entities each seed draws.
const ENTITIES: usize = 20;

/// The filters each seed draws.
const FILTERS: usize = 30;

/// Creates the Entities `seed` draws under a namespace of its own, then changes them, and expects Query Entities and
/// a Subscription with each filter it draws to agree on the Entities selected after each round, answering what they
/// selected.
async fn expect_drawn_case_to_agree(seed: u64) -> Result<Selections> {
    let case = GeneratedCase::drawn(seed, ENTITIES, FILTERS)?;
    expect_generated_case_to_agree(&broker::front()?, &Namespace::fresh(), seed, &case).await
}

/// Pins: for 64 fixed seeds, each drawing 20 Entities over eight Attribute names of every attribute family, some with
/// Relationships to Entities drawn before them, and 30 distinct filters drawn from those Entities, 1920 filters in
/// all, Query Entities and a Subscription with each filter, its `jsonKeys` and `expandValues`, a Linked Entity hop
/// asked with `join` and `joinLevel`, and a geoquery some filters carry, select the same Entities after the Entities
/// are created and again after a second write changes each; clause 4.9 gives a Query Term one meaning wherever it is
/// evaluated. In each round, summed over the seeds, at least 65% of the filters select an Entity, the median filter
/// that selects any selects at least 3, at most 10% select every Entity of their case, and at most 5% of the answers
/// of several Entities repeat an earlier answer of their case, so the agreement is over selections that differ
/// Clause: 4.9, 4.10, 5.7.2.4, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9216_generated_filters_select_alike_in_a_query_and_a_subscription() -> Result<()> {
    let mut selections = Selections::default();
    for outcome in join_all((0..SEEDS).map(expect_drawn_case_to_agree)).await {
        selections.absorb(outcome?);
    }
    selections.expect_within(SelectionBounds::Q);
    Ok(())
}
