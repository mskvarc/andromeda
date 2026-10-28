//! Annex C.5.6: the speed evolution of the Vehicle that is not a Mercedes as value and time pairs (clauses 4.5.9 and
//! 5.7.4).

use crate::{
    printed_document::expect_flat,
    served_context::{ServedContexts, linked, ok_body},
    speed_evolution::{SPEEDS, record_b9211},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{q::Query, temporal_query},
};
use serde_json::{Value, json};

/// Pins: Query Temporal Evolution of Entities with format=temporalValues answers the speed evolution of B9211 as
/// value and time pairs as C.5.6 prints them
/// Clause: C.5.6, 4.5.9, 6.3.12, 5.7.4
#[gtest]
#[tokio::test]
async fn c_5_6_0001_the_temporal_values_of_the_speed_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let id = record_b9211(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query_values(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .q(r#"brandName!="Mercedes""#.parse::<Query>()?)
            .attrs(vec!["speed".parse()?])
            .send()
            .await,
    )?;
    let values: Vec<Value> = SPEEDS.iter().map(|(value, at)| json!([value, at])).collect();
    expect_flat(
        body,
        vec![json!({
            "id": id.to_string(),
            "type": namespace.suffixed("Vehicle"),
            "speed": {"type": "Property", "values": values},
        })],
    )?;
    served.stop();
    Ok(())
}
