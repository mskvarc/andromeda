//! ETSI `021_16`: the temporal evolutions of the Vehicles are queried by a POST whose `Query` body names the entity type
//! and the temporal relation, and the answer is paginated as partial content (clauses 5.7.3 and 6.3.10).

use crate::suite_keywords::{content_range, create_vehicle_twenty_pair, delete_vehicle_twenty_pair};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    query::temporal_query,
};

/// Pins: Check that temporal pagination is triggered on the post temporal query
/// Clause: 5.7.3, 6.3.10
/// Corrected: fix(content-range): temporal content-range unit is datetime (6.3.10)
#[gtest]
#[tokio::test]
async fn e021_16_01_retrieve_the_entities_via_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_twenty_pair(&broker, &namespace).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::before("2020-08-02T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::PARTIAL_CONTENT))?;
    let received = received?;
    let range = content_range(received.head.headers()).or_fail()?;
    expect_that!(range.unit, eq("DateTime"));

    delete_vehicle_twenty_pair(&broker, &namespace).await?;
    Ok(())
}
