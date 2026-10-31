//! An inline temporal join nests a chain of 1000 linked entities.

use andromeda_test_suite::{
    broker,
    linked_temporal_fixtures::{send_joined, vehicle_chain},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::QueryTemporalEvolution,
    model::Lossless,
    query::{Join, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: a temporal query with join=inline and `joinLevel=1000` over a chain of 1000 Relationships, each observed
/// once inside the window, answers 200 with every one of the 1000 Linked Entities nested under the Relationship
/// instance before it, down to the last
/// Clause: 4.5.23.1, 4.5.23.2, 5.7.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// where Table 6.5.3.1-1 sets no upper bound on `joinLevel`. The chain is written as current state, which this
/// broker records as history
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0016_an_inline_temporal_join_nests_a_chain_of_1000_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let chain = vehicle_chain(&namespace, 1000)?;
    broker.entities().batch_create(&chain).send().await?.done()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let ids = [namespace.urn("Vehicle", 0)?];
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .ids(&ids)
        .format(Lossless::Normalized)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query it writes. The answer nests the chain
    // 1000 deep, which no model reads, so the nested linked entities are counted in the text.
    let answer = send_joined(&broker, &query, Join::Inline, 1000).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let body = String::from_utf8_lossy(answer.body());
    expect_that!(body.matches(r#""entity":"#).count(), eq(1000));
    expect_that!(body.contains(&format!(r#""id":"{}""#, namespace.urn("Vehicle", 999)?)), eq(true));
    expect_that!(body.contains(&format!(r#""id":"{}""#, namespace.urn("Vehicle", 1000)?)), eq(true));
    Ok(())
}
