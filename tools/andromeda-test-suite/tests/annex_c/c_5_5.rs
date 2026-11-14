//! Annex C.5.5: the temporal evolution of the speed of the Vehicle that is not a Mercedes over a one-hour window
//! (clauses 4.5.7 and 5.7.4).

use crate::{
    printed_document::{expect_flat, remove_from_items},
    served_context::{ServedContexts, linked, ok_body},
    speed_evolution::{BrandObserved, SPEEDS, record_b9211},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, evolution::TemporalAttributeName},
    query::{q::Query, temporal_query},
};
use serde_json::{Value, json};

/// Queries the evolution of the Vehicles that are not a Mercedes between 12:00 and 13:00, reduced to `attrs`, and
/// answers the body with every `instanceId` of B9211's speed removed.
async fn not_a_mercedes(broker: &BrokerClient, namespace: &Namespace, attrs: Vec<TemporalAttributeName>) -> Result<Value> {
    let mut body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .q(r#"brandName!="Mercedes""#.parse::<Query>()?)
            .attrs(attrs)
            .send()
            .await,
    )?;
    // An instance may carry its instanceId or not (clause 4.5.7); the annex prints none.
    remove_from_items(&mut body, "/0/speed", "instanceId");
    Ok(body)
}

/// The speed evolution of B9211 as the annex prints it, without an `@context`.
fn printed_speed(namespace: &Namespace) -> Value {
    let speed: Vec<Value> = SPEEDS
        .iter()
        .map(|(value, at)| json!({"type": "Property", "value": value, "observedAt": at}))
        .collect();
    json!({"id": namespace.urn_text("Vehicle", "B9211"), "type": namespace.suffixed("Vehicle"), "speed": speed})
}

/// Pins: Query Temporal Evolution of Entities with type, q, attrs=speed and a one-hour window answers the three speed
/// instances of B9211 as C.5.5 EXAMPLE 1 prints them
/// Clause: C.5.5, 4.5.7, 4.11, 5.7.4
/// Expected: B9211's `brandName` is observed at 12:00, inside the window, where the annex observes it at 11:00; a
/// query is checked only against the instances inside the window (clauses 4.11 and 5.7.4.4)
#[gtest]
#[tokio::test]
async fn c_5_5_0001_the_speed_evolution_of_the_vehicle_not_a_mercedes() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    record_b9211(&broker, &namespace, BrandObserved::AtWindowStart).await?;
    let body = not_a_mercedes(&broker, &namespace, vec!["speed".parse()?]).await?;
    expect_flat(body, vec![printed_speed(&namespace)])?;
    served.stop();
    Ok(())
}

/// Pins: Query Temporal Evolution of Entities with attrs=speed,brandName, over B9211 whose one brandName instance is
/// observed at 11:00, before the window, answers no Vehicle
/// Clause: C.5.5, 4.5.7, 4.11, 5.7.4
/// Expected: the annex answers B9211's speed instances, while a query is checked only against the instances inside
/// the window (clauses 4.11 and 5.7.4.4), and brandName has none there
#[gtest]
#[tokio::test]
async fn c_5_5_0002_a_brand_name_observed_before_the_window_selects_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    record_b9211(&broker, &namespace, BrandObserved::BeforeWindow).await?;
    let body = not_a_mercedes(&broker, &namespace, vec!["speed".parse()?, "brandName".parse()?]).await?;
    expect_flat(body, Vec::new())?;
    served.stop();
    Ok(())
}
