//! A q on instance id selects nothing in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property};

/// Pins: for a Sensor holding a `reading`, Query Entities and a Subscription with q `reading.instanceId`,
/// `reading.instanceId=="x"` and `reading.instanceId!="x"` both select nothing, while q `reading` selects the Sensor;
/// table 5.2.5-2 has `instanceId` only in the temporal representation, so the Entity holds no target element for
/// the path
/// Clause: 4.9, 5.2.5, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9082_a_q_on_instance_id_selects_nothing_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let instance = namespace.urn_text("Instance", 1);
    let entities = [Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(5))
        .build()?];
    let equal = format!("reading.instanceId==\"{instance}\"");
    let unequal = format!("reading.instanceId!=\"{instance}\"");
    let cases = [
        QCase {
            q: "reading",
            selects: &[&sensor_id],
        },
        QCase {
            q: "reading.instanceId",
            selects: &[],
        },
        QCase { q: &equal, selects: &[] },
        QCase { q: &unequal, selects: &[] },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &sensor_type, &entities, &cases).await
}
