//! Generated geoqueries select alike in a query and a subscription.

use andromeda_test_suite::{broker, geo_generation::GeneratedGeoCase, geo_parity::expect_generated_geo_case_to_agree, namespace::Namespace};
use futures::future::join_all;
use googletest::prelude::*;

/// The seeds the cases are drawn from, each drawing the same case on every run.
const SEEDS: u64 = 32;

/// The Entities each seed draws.
const ENTITIES: usize = 12;

/// The geoqueries each seed draws.
const QUERIES: usize = 24;

/// Creates the Entities `seed` draws under a namespace of its own and expects Query Entities and a Subscription with
/// each geoquery it draws to agree on the Entities selected.
async fn expect_drawn_case_to_agree(seed: u64) -> Result<()> {
    let case = GeneratedGeoCase::drawn(seed, ENTITIES, QUERIES)?;
    expect_generated_geo_case_to_agree(&broker::front()?, &Namespace::fresh(), seed, &case).await
}

/// Pins: for 32 fixed seeds, each locating 12 Entities at points, lines and polygons on both sides of the
/// antimeridian and across it, in a default `location` instance, one in a dataset, both or none, and some at a second
/// `GeoProperty` `area`, and drawing 24 geoqueries over the same geometries, 768 geoqueries in all, Query Entities
/// and a Subscription with each geoquery select the same Entities; clause 4.10 gives a geoquery one meaning wherever
/// it is evaluated
/// Clause: 4.10, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_qry_geo_9102_generated_geoqueries_select_alike_in_a_query_and_a_subscription() -> Result<()> {
    for outcome in join_all((0..SEEDS).map(expect_drawn_case_to_agree)).await {
        outcome?;
    }
    Ok(())
}
