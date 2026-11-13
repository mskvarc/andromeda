//! A q on dataset id selects by the dataset of an instance in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property};

/// Pins: for a Sensor holding a default `reading` and one of dataset `d1`, a Sensor holding one of dataset `d2` and a
/// Sensor holding only a default one, Query Entities and a Subscription with q `reading.datasetId=="d1"` both select
/// the first alone, `reading.datasetId==d2` with the unquoted URI the second alone, `!="d1"` the second alone (a
/// default instance has no `datasetId`) and the bare path the first two
/// Clause: 4.5.5, 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9081_a_q_on_dataset_id_selects_by_the_instance_dataset_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let first_dataset = namespace.dataset_id("Dataset", "d1")?;
    let second_dataset = namespace.dataset_id("Dataset", "d2")?;
    let both_id = namespace.urn("Sensor", "both")?;
    let second_id = namespace.urn("Sensor", "second")?;
    let default_id = namespace.urn("Sensor", "default")?;
    let entities = [
        Entity::builder()
            .id(default_id)
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(8))
            .build()?,
        Entity::builder()
            .id(second_id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(7).dataset_id(second_dataset.clone()))
            .build()?,
        Entity::builder()
            .id(both_id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(5))
            .property("reading", Property::builder(6).dataset_id(first_dataset.clone()))
            .build()?,
    ];
    let first = format!("reading.datasetId==\"{first_dataset}\"");
    let second = format!("reading.datasetId=={second_dataset}");
    let not_first = format!("reading.datasetId!=\"{first_dataset}\"");
    let cases = [
        QCase {
            q: &first,
            selects: &[&both_id],
        },
        QCase {
            q: &second,
            selects: &[&second_id],
        },
        QCase {
            q: &not_first,
            selects: &[&second_id],
        },
        QCase {
            q: "reading.datasetId",
            selects: &[&both_id, &second_id],
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &sensor_type, &entities, &cases).await
}
