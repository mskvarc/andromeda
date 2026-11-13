//! A q on value type selects by the declared type in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property, VocabTerm};

/// Pins: for Sensors whose `reading` declares the valueType `Text`, `Count`, and none, Query Entities and a
/// Subscription with q `reading.valueType=="Text"` both select the first alone, `!="Text"` the second alone, the bare
/// path and the value list of both types the two typed ones, and `~="Text"` the first, the operand expanded like the
/// declared type
/// Clause: 4.5.2, 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9079_a_q_on_value_type_selects_by_the_declared_type_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let text = namespace.suffixed("Text");
    let count = namespace.suffixed("Count");
    let text_id = namespace.urn("Sensor", "text")?;
    let count_id = namespace.urn("Sensor", "count")?;
    let untyped_id = namespace.urn("Sensor", "untyped")?;
    let entities = [
        Entity::builder()
            .id(untyped_id)
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder("5"))
            .build()?,
        Entity::builder()
            .id(count_id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(5).value_type(count.parse::<VocabTerm>()?))
            .build()?,
        Entity::builder()
            .id(text_id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder("5").value_type(text.parse::<VocabTerm>()?))
            .build()?,
    ];
    let equal = format!("reading.valueType==\"{text}\"");
    let unequal = format!("reading.valueType!=\"{text}\"");
    let listed = format!("reading.valueType==\"{text}\",\"{count}\"");
    let pattern = format!("reading.valueType~=\"{text}\"");
    let cases = [
        QCase {
            q: &equal,
            selects: &[&text_id],
        },
        QCase {
            q: &unequal,
            selects: &[&count_id],
        },
        QCase {
            q: "reading.valueType",
            selects: &[&text_id, &count_id],
        },
        QCase {
            q: &listed,
            selects: &[&text_id, &count_id],
        },
        QCase {
            q: &pattern,
            selects: &[&text_id],
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &sensor_type, &entities, &cases).await
}
