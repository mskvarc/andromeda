//! Annex C.5.14: the temporal evolution of the Vehicle speed aggregated by maximum and average over four-minute
//! periods (clauses 4.5.19 and 5.7.4).

use crate::{
    printed_document::{as_floats, expect_flat},
    served_context::{ServedContexts, linked, ok_body},
    speed_evolution::record_b9211,
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::{q::Query, temporal_query},
};
use serde_json::json;

/// Pins: Query Temporal Evolution of Entities with aggrMethods=max,avg over PT4M periods answers the aggregated speed
/// of B9211 as C.5.14 prints
/// Clause: C.5.14, 4.5.19, 6.3.12, 5.7.4
#[gtest]
#[tokio::test]
async fn c_5_14_0001_the_aggregated_speed_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let id = record_b9211(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query_aggregated(
                temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?)
                    .aggregation_methods([AggregationMethod::Max, AggregationMethod::Avg])
                    .aggregation_period("PT4M".parse()?),
            )
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .q(r#"brandName!="Mercedes""#.parse::<Query>()?)
            .attrs(vec!["speed".parse()?])
            .send()
            .await,
    )?;
    // The annex prints whole numbers; an average may be written with a fraction, which the comparison ignores.
    expect_flat(
        as_floats(&body),
        vec![json!({
            "id": id.to_string(),
            "type": namespace.suffixed("Vehicle"),
            "speed": {
                "type": "Property",
                "max": [[120.0, "2018-08-01T12:00:00Z", "2018-08-01T12:04:00Z"], [100.0, "2018-08-01T12:04:00Z", "2018-08-01T12:08:00Z"]],
                "avg": [[120.0, "2018-08-01T12:00:00Z", "2018-08-01T12:04:00Z"], [90.0, "2018-08-01T12:04:00Z", "2018-08-01T12:08:00Z"]],
            },
        })],
    )?;
    served.stop();
    Ok(())
}
