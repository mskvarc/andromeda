//! A trailing path after an instance member addresses nothing in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property};

/// Pins: for a Vehicle whose `speed` is 80, observed at 2024-01-01T00:00:00Z, Query Entities and a Subscription with
/// q speed.observedAt and with q speed.observedAt[*]>2023-12-31T00:00:00Z select it, and with q speed.observedAt[x]
/// and q speed.observedAt[x]!=2024-01-01T00:00:00Z select nothing; a trailing path is evaluated against the target
/// value, a `DateTime` has no member, and a target element that does not resolve is non-existent for every operator
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9087_a_trailing_path_after_an_instance_member_addresses_nothing_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?];
    let cases = [
        QCase {
            q: "speed.observedAt",
            selects: &[&vehicle_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.observedAt[*]>2023-12-31T00:00:00Z",
            selects: &[&vehicle_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.observedAt[x]",
            selects: &[],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.observedAt[x]!=2024-01-01T00:00:00Z",
            selects: &[],
            request: QRequest::PLAIN,
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &vehicle_type, &entities, &cases).await
}
