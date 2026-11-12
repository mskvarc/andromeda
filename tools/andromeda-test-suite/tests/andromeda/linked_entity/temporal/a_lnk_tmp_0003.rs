//! A flat join appends the linked entity as history over the window.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    linked_temporal_fixtures::{ParkedVehicles, send_joined},
    namespace::Namespace,
    nth::nth,
    temporal_page_body::entities,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{NormalizedTemporal, QueryTemporalEvolution},
    query::{Join, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: a temporal query with join=flat and `joinLevel=1` answers the root and, after it, the entity its in-window
/// Relationship instances name, rendered as the history each of its attributes had over the same window
/// Clause: 4.5.23, 5.7.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0003_a_flat_join_appends_the_linked_entity_as_history_over_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicles = ParkedVehicles::new(&namespace)?;
    vehicles.record_speeds(&broker, &vehicles.b, 30, 40).await?;
    vehicles.record_a(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&vehicles.vehicle_type).build()?;
    let ids = [vehicles.a.clone()];
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .format(NormalizedTemporal)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query it writes.
    let answer = send_joined(&broker, &query, Join::Flat, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let page = entities(&answer)?;
    expect_that!(page, len(eq(2)));
    expect_that!(page, nth(0, has_id(&vehicles.a)));
    expect_that!(
        page,
        nth(
            1,
            all!(
                has_id(&vehicles.b),
                has_evolution(
                    "speed",
                    all!(
                        len(eq(2)),
                        nth(0, property().value(30).observed_at("2020-08-01T12:03:00Z".parse()?)),
                        nth(1, property().value(40).observed_at("2020-08-01T12:05:00Z".parse()?))
                    )
                )
            )
        )
    );
    Ok(())
}
