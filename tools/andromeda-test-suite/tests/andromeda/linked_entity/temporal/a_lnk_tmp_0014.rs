//! The entity map of a joined temporal query maps the linked entities.

use andromeda_test_suite::{
    broker,
    linked_temporal_fixtures::{OwnedBuilding, send_joined},
    namespace::Namespace,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, NormalizedTemporal, QueryTemporalEvolution},
    },
    model::NgsiLdMediaType,
    query::{Join, builders::SelectorTypeBuilder, temporal_query},
    registration::EntityMapId,
};

/// Pins: a joined temporal query with `entityMap=true` answers 201 naming an `EntityMap` that maps the root and the
/// Linked Entity it reached
/// Clause: 4.5.23, 5.5.14, 6.18.3.2, 6.32.3.1
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clauses 4.5.23
/// and 4.21, over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0014_the_entity_map_of_a_joined_temporal_query_maps_the_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let graph = OwnedBuilding::new(&namespace)?;
    graph.record(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&graph.building_type).build()?;
    let ids = [graph.building.clone()];
    let entity_map = EntityMapRequest::returned();
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .entity_map(&entity_map)
        .format(NormalizedTemporal)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query it writes. The answer is the 201 that
    // names the map, which the typed page does not keep.
    let answer = send_joined(&broker, &query, Join::Flat, 1).await?;
    expect_that!(answer.status(), eq(StatusCode::CREATED));
    let map = answer
        .headers()
        .get("NGSILD-EntityMap")
        .and_then(|value| value.to_str().ok())
        .or_fail()?
        .parse::<EntityMapId>()?;
    let mapped = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map)
        .send()
        .await?;
    expect_that!(mapped.entity_map.contains_key(&graph.building), eq(true));
    expect_that!(mapped.entity_map.contains_key(&graph.person), eq(true));
    Ok(())
}
