//! A pick into a linked entity needs a retrieving join.

use andromeda_test_suite::{
    body_json::{json_count, json_exists},
    broker,
    linked_temporal_fixtures::{ParkedVehicles, send_joined},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType},
    query::{Join, builders::SelectorTypeBuilder, projection::Projection, temporal_query},
};

/// Pins: a temporal query whose pick names a member of a Linked Entity answers 400 `BadRequestData` without a join,
/// and 200 with join=flat, the root narrowed to the Relationship and the linked entity to the picked member
/// Clause: 4.21, 4.5.23, 5.7.2.4, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0008_a_pick_into_a_linked_entity_needs_a_retrieving_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicles = ParkedVehicles::new(&namespace)?;
    vehicles.record_speeds(&broker, &vehicles.b, 30, 40).await?;
    vehicles.record_a(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&vehicles.vehicle_type).build()?;
    let ids = [vehicles.a.clone()];
    let pick = "isParkedIn{speed}".parse::<Projection>()?;
    let query = QueryTemporalEvolution::projected_builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .pick(&pick)
        .format(Lossless::Normalized)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let refused = client.send_unchecked(client.unchecked(&query)?).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The temporal query writes no `join`, so the parameters follow the query it writes. The root narrowed to the
    // Relationship and the linked entity to the picked member have neither `id` nor `type`, which no model reads whole.
    let answer = send_joined(&broker, &query, Join::Flat, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let body = answer.body();
    expect_that!(body, json_count("$", 2));
    expect_that!(body, json_count("$[0].isParkedIn", 2));
    expect_that!(body, json_exists("$[?count($[0].*)==1]"));
    expect_that!(body, json_count("$[1].speed", 2));
    expect_that!(body, json_exists("$[?count($[1].*)==1]"));
    Ok(())
}
