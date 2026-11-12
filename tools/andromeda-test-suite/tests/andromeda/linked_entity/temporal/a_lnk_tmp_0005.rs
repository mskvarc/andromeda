//! A relationship instance outside the window names no linked entity.

use andromeda_test_suite::{
    broker,
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

/// Pins: a Relationship instance outside the window of a flat temporal join is not followed, so the entity it alone
/// names is not answered although it has history inside the window
/// Clause: 4.5.23, 5.7.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0005_a_relationship_instance_outside_the_window_names_no_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicles = ParkedVehicles::new(&namespace)?;
    vehicles.record_speeds(&broker, &vehicles.b, 30, 40).await?;
    vehicles.record_speeds(&broker, &vehicles.c, 30, 40).await?;
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
    expect_that!(page, nth(1, has_id(&vehicles.b)));
    Ok(())
}
