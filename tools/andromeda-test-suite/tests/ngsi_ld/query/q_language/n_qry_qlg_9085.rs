//! A range against a Relationship is unequal in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Relationship};

/// Pins: for a Vehicle whose Relationship `r` targets a URI, Query Entities and a Subscription with q r!=1..5 select
/// it and with q r==1..5 select nothing; a range is a value of the equal and unequal productions, which are the two
/// operators a Relationship admits, and a URI object and a Number range are of different data types, so they are
/// unequal and not equal
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9085_a_range_against_a_relationship_is_unequal_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let parking_id = namespace.urn("Parking", 1)?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .relationship("r", Relationship::builder(parking_id))
        .build()?];
    let cases = [
        QCase {
            q: "r!=1..5",
            selects: &[&vehicle_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "r==1..5",
            selects: &[],
            request: QRequest::PLAIN,
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &vehicle_type, &entities, &cases).await
}
