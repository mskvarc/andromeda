//! A segment naming a Property's value member addresses nothing in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Value};

/// Pins: for a Vehicle whose `speed` is 80 and whose `address` value holds the member `city` "Berlin", Query Entities
/// and a Subscription with q speed==80 and with q address[city]=="Berlin" select it, and with q speed.value,
/// speed.value==80, speed.value!=50 and address.value.city=="Berlin" select nothing; each name after the first of an
/// attribute path is a sub-Property or sub-Relationship, a Property's Value is neither, and a compound value is
/// entered through the trailing path in square brackets
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9086_a_segment_naming_the_value_member_addresses_nothing_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 80)
        .property("address", object([("city", Value::from("Berlin"))]))
        .build()?];
    let cases = [
        QCase {
            q: "speed==80",
            selects: &[&vehicle_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "address[city]==\"Berlin\"",
            selects: &[&vehicle_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.value",
            selects: &[],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.value==80",
            selects: &[],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "speed.value!=50",
            selects: &[],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "address.value.city==\"Berlin\"",
            selects: &[],
            request: QRequest::PLAIN,
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &vehicle_type, &entities, &cases).await
}
