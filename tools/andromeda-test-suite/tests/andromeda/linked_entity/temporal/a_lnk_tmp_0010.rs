//! An inline join nests a copy narrowed per relationship.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals, json_exists},
    broker,
    linked_temporal_fixtures::{OwnedBuilding, send_joined},
    namespace::Namespace,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    query::{Join, projection::Projection, temporal_query},
};

/// Pins: an inline temporal retrieve with pick=owner{name},tenant,locatedIn{size} nests under every in-window owner
/// instance the Person narrowed to name, under every tenant instance the whole Person, and under no `locatedIn`
/// instance the City its group empties
/// Clause: 4.21, 4.5.23, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clauses 4.5.23
/// and 4.21, over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0010_an_inline_join_nests_a_copy_narrowed_per_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let graph = OwnedBuilding::new(&namespace)?;
    graph.record(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let pick = "owner{name},tenant,locatedIn{size}".parse::<Projection>()?;
    let retrieval = RetrieveTemporalEvolution::projected_builder()
        .id(&graph.building)
        .selection(TemporalSelection::Query(&window))
        .pick(&pick)
        .format(NormalizedTemporal)
        .build();
    // The temporal retrieval writes no `join`, so the parameters follow the query it writes. Each instance nests a
    // narrowed copy of its linked entity, which no model reads, so the answer is read as received.
    let answer = send_joined(&broker, &retrieval, Join::Inline, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let body = answer.body();
    expect_that!(body, json_count("$.owner", 2));
    expect_that!(body, json_count("$.owner[0].entity.name", 2));
    expect_that!(body, json_exists("$[?length($.owner[0].entity)==1]"));
    expect_that!(body, json_count("$.owner[1].entity.name", 2));
    expect_that!(body, json_exists("$[?length($.owner[1].entity)==1]"));
    expect_that!(body, json_count("$.tenant", 2));
    expect_that!(body, json_equals("$.tenant[0].entity.id", graph.person.to_string()));
    expect_that!(body, json_count("$.tenant[0].entity.name", 2));
    expect_that!(body, json_count("$.tenant[0].entity.email", 2));
    expect_that!(body, json_count("$.tenant[0].entity.employer", 2));
    expect_that!(body, json_count("$.tenant[1].entity.name", 2));
    expect_that!(body, json_count("$.tenant[1].entity.email", 2));
    expect_that!(body, json_count("$.locatedIn", 2));
    expect_that!(body, json_equals("$.locatedIn[0].object", graph.city.to_string()));
    expect_that!(body, json_absent("$.locatedIn[0].entity"));
    expect_that!(body, json_equals("$.locatedIn[1].object", graph.city.to_string()));
    expect_that!(body, json_absent("$.locatedIn[1].entity"));
    Ok(())
}
