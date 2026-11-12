//! An inline join nests the linked entity under every in window instance.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    linked_temporal_fixtures::{ParkedVehicles, send_joined},
    namespace::Namespace,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{NormalizedTemporal, QueryTemporalEvolution},
    query::{Join, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: a temporal query with join=inline and `joinLevel=1` answers only the root, and nests under every in-window
/// instance of its Relationship the linked entity with its own history over the window
/// Clause: 4.5.23, 5.7.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0004_an_inline_join_nests_the_linked_entity_under_every_in_window_instance() -> Result<()> {
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
    // The temporal query writes no `join`, so the parameters follow the query it writes. Vehicle `a` nests the linked
    // vehicle under each in-window instance, which no model reads, so the answer is read as received.
    let answer = send_joined(&broker, &query, Join::Inline, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let body = answer.body();
    expect_that!(body, json_count("$", 1));
    expect_that!(body, json_equals("$[0].id", vehicles.a.to_string()));
    expect_that!(body, json_count("$[0].isParkedIn", 2));
    for instance in 0..2 {
        expect_that!(body, json_equals(&format!("$[0].isParkedIn[{instance}].entity.id"), vehicles.b.to_string()));
        expect_that!(body, json_count(&format!("$[0].isParkedIn[{instance}].entity.speed"), 2));
        expect_that!(
            body,
            json_equals(&format!("$[0].isParkedIn[{instance}].entity.speed[0].observedAt"), "2020-08-01T12:03:00Z")
        );
        expect_that!(
            body,
            json_equals(&format!("$[0].isParkedIn[{instance}].entity.speed[1].observedAt"), "2020-08-01T12:05:00Z")
        );
    }
    Ok(())
}
