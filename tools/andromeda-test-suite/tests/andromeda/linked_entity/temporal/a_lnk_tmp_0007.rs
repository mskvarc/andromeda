//! A q hop without a retrieving join is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::{q, temporal_query},
};

/// Pins: a temporal query whose q traverses a Relationship into a Linked Entity without a join that retrieves
/// Linked Entities answers 400 `BadRequestData`
/// Clause: 4.9, 4.5.23, 5.7.2.4, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
/// temporal operations; the `temporal_linked_retrieval` extension admits it under the Core rules of clause 4.5.23,
/// over the window of the request
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0007_a_q_hop_without_a_retrieving_join_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:10:00Z".parse()?))
        .entity_type(&namespace.entity_type("Vehicle")?)
        .q(q::attr("isParkedIn").linked("speed").gt(5))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
