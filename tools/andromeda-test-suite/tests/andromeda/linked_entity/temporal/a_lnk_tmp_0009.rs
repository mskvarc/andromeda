//! A flat join narrows the linked entity by the core rules.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_exists},
    broker,
    linked_temporal_fixtures::{OwnedBuilding, send_joined},
    namespace::Namespace,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::QueryTemporalEvolution,
    model::Lossless,
    query::{Join, builders::SelectorTypeBuilder, projection::Projection, temporal_query},
};

/// Pins: a flat temporal join with pick=id,owner{name} answers the Building narrowed to id and owner and, after it,
/// the owner narrowed to its name history alone, without id or type
/// Clause: 4.21, 4.5.23, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clauses 4.5.23
/// and 4.21, over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0009_a_flat_join_narrows_the_linked_entity_by_the_core_rules() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let graph = OwnedBuilding::new(&namespace)?;
    graph.record(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&graph.building_type).build()?;
    let ids = [graph.building.clone()];
    let pick = "id,owner{name}".parse::<Projection>()?;
    let query = QueryTemporalEvolution::projected_builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .pick(&pick)
        .format(Lossless::Normalized)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query it writes. The narrowed entities have no
    // `id` or `type`, which no model reads whole, so the answer is read as received.
    let answer = send_joined(&broker, &query, Join::Flat, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let body = answer.body();
    expect_that!(body, json_count("$", 2));
    expect_that!(body, json_equals("$[0].id", graph.building.to_string()));
    expect_that!(body, json_count("$[0].owner", 2));
    expect_that!(body, json_exists("$[?count($[0].*)==2]"));
    expect_that!(body, json_count("$[1].name", 2));
    expect_that!(body, json_equals("$[1].name[0].value", "Ada"));
    expect_that!(body, json_exists("$[?count($[1].*)==1]"));
    Ok(())
}
