//! Generated geoqueries select alike in a query and a subscription.

use andromeda_test_suite::{
    broker,
    drawn_selection::{SelectionBounds, Selections},
    geo_generation::GeneratedGeoCase,
    geo_parity::expect_generated_geo_case_to_agree,
    namespace::Namespace,
};
use futures::future::join_all;
use googletest::prelude::*;

/// The seeds the cases are drawn from, each drawing the same case on every run.
const SEEDS: u64 = 32;

/// The Entities each seed draws.
const ENTITIES: usize = 12;

/// The geoqueries each seed draws.
const QUERIES: usize = 24;

/// Creates the Entities `seed` draws under a namespace of its own, then moves them, and expects Query Entities and a
/// Subscription with each geoquery it draws to agree on the Entities selected after each round, answering what they
/// selected.
async fn expect_drawn_case_to_agree(seed: u64) -> Result<Selections> {
    let case = GeneratedGeoCase::drawn(seed, ENTITIES, QUERIES)?;
    expect_generated_geo_case_to_agree(&broker::front()?, &Namespace::fresh(), seed, &case).await
}

/// Pins: for 32 fixed seeds, each locating 12 Entities at points, lines and polygons on both sides of the
/// antimeridian and across it, in a default `location` instance, one in a dataset, both or none, and some at a second
/// `GeoProperty` `area`, no two at one `location` geometry, then moving each within its region, and drawing 24
/// geoqueries, no two reaching the same way from one region under one `GeoProperty`, their reference geometry drawn for
/// its relation from the pool or from the geometries the Entities hold, 768 geoqueries in all, Query Entities and a
/// Subscription with each geoquery select the same Entities after the Entities are created and again after they are
/// moved; clause 4.10 gives a geoquery one meaning wherever it is evaluated. In each round, summed over the seeds, at
/// least 65% of the geoqueries select an Entity, the median geoquery that selects any selects at least 2, at most 10%
/// select every Entity of their case, and at most 5% of the answers of several Entities repeat an earlier answer of
/// their case, so the agreement is over selections that differ
/// Clause: 4.10, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_geo_9102_generated_geoqueries_select_alike_in_a_query_and_a_subscription() -> Result<()> {
    let mut selections = Selections::default();
    for outcome in join_all((0..SEEDS).map(expect_drawn_case_to_agree)).await {
        selections.absorb(outcome?);
    }
    selections.expect_within(SelectionBounds::GEO);
    Ok(())
}
