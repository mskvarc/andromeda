//! A pick deeper than join level is bad request data.

use andromeda_test_suite::{broker, linked_temporal_fixtures::OwnedBuilding, namespace::Namespace, problem::problem, query_join::with_join};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType},
    query::{Join, builders::SelectorTypeBuilder, projection::Projection, temporal_query},
};

/// Pins: a temporal query whose pick reaches two Relationships deep under `joinLevel=1` answers 400
/// `BadRequestData`
/// Clause: 4.21, 4.5.23, 5.7.1.4, 5.7.2.4
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clauses 4.5.23
/// and 4.21, over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0013_a_pick_deeper_than_join_level_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let graph = OwnedBuilding::new(&namespace)?;
    graph.record(&broker).await?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&graph.building_type).build()?;
    let ids = [graph.building.clone()];
    let pick = "owner{employer{name}}".parse::<Projection>()?;
    let query = QueryTemporalEvolution::projected_builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .pick(&pick)
        .format(Lossless::Normalized)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The temporal query writes no `join`, so the parameters follow the query it writes.
    let request = with_join(client.unchecked(&query)?, Join::Flat, 1)?;
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
