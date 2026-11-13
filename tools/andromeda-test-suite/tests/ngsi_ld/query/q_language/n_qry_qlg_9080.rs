//! A q on object type selects by any declared type in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Relationship, VocabTerm};

/// Pins: for Sensors whose `feeds` Relationship declares the objectType `Device`, both `Device` and `Gauge`, `Valve`,
/// and none, Query Entities and a Subscription with q `feeds.objectType=="Device"` both select the first two,
/// `=="Gauge"` the second, `!="Device"` the third, `=="Gauge","Valve"` the second and third, the bare path the three
/// typed ones, and `~="Gauge"` none, since an array of types is no String
/// Clause: 4.5.3, 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9080_a_q_on_object_type_selects_by_any_declared_type_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let device_type = namespace.entity_type("Device")?;
    let gauge_type = namespace.entity_type("Gauge")?;
    let valve_type = namespace.entity_type("Valve")?;
    let device_id = namespace.urn("Sensor", "device")?;
    let both_id = namespace.urn("Sensor", "both")?;
    let valve_id = namespace.urn("Sensor", "valve")?;
    let untyped_id = namespace.urn("Sensor", "untyped")?;
    let target_id = namespace.urn("Device", 1)?;
    let mut entities = vec![
        Entity::builder()
            .id(untyped_id)
            .entity_type(sensor_type.clone())
            .relationship("feeds", Relationship::builder(target_id.clone()))
            .build()?,
    ];
    for (id, object_types) in [
        (&valve_id, vec![VocabTerm::from(&valve_type)]),
        (&device_id, vec![VocabTerm::from(&device_type)]),
        (&both_id, vec![VocabTerm::from(&device_type), VocabTerm::from(&gauge_type)]),
    ] {
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(sensor_type.clone())
                .relationship("feeds", Relationship::builder(target_id.clone()).object_type(object_types))
                .build()?,
        );
    }
    let device = format!("feeds.objectType==\"{device_type}\"");
    let gauge = format!("feeds.objectType==\"{gauge_type}\"");
    let not_device = format!("feeds.objectType!=\"{device_type}\"");
    let gauge_or_valve = format!("feeds.objectType==\"{gauge_type}\",\"{valve_type}\"");
    let cases = [
        QCase {
            q: &device,
            selects: &[&device_id, &both_id],
        },
        QCase {
            q: &gauge,
            selects: &[&both_id],
        },
        QCase {
            q: &not_device,
            selects: &[&valve_id],
        },
        QCase {
            q: &gauge_or_valve,
            selects: &[&both_id, &valve_id],
        },
        QCase {
            q: "feeds.objectType",
            selects: &[&device_id, &both_id, &valve_id],
        },
        QCase {
            q: "feeds.objectType~=\"Gauge\"",
            selects: &[],
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &sensor_type, &entities, &cases).await
}
