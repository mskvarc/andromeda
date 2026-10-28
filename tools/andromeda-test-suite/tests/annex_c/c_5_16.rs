//! Annex C.5.16: a temporal query with a scope query answers the scope and speed evolution of the Vehicles in that
//! scope, the attributes selected by `attrs` and again by `pick` (clauses 4.18 and 4.5.9).

use crate::{
    printed_document::expect_flat,
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        DateTime,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Scope,
        evolution::{ScopeInstance, TemporalEntity},
    },
    query::{projection, scope_query, temporal_query},
};
use serde_json::{Value, json};

/// Records the evolution of the Vehicle `suffix`: its `scopes` observed at `scoped_at`, then a speed at each of
/// `speeds`.
async fn record(client: &BrokerClient, namespace: &Namespace, suffix: &str, scopes: &[&str], scoped_at: &str, speeds: [(i64, &str); 3]) -> Result<()> {
    let scopes = scopes.iter().map(|scope| scope.parse::<Scope>()).collect::<Result<Vec<_>, _>>()?;
    let mut evolution = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .scope(ScopeInstance::new(OneOrMany::try_from(scopes)?).with_observed_at(scoped_at.parse::<DateTime>()?));
    for (speed, at) in speeds {
        evolution = evolution.property("speed", Property::builder(speed).observed_at(at.parse::<DateTime>()?));
    }
    client.temporal().create_or_update(&evolution.build()?).send().await?;
    Ok(())
}

/// The two Vehicles in `/Madrid/Centro`, in the simplified temporal representation.
fn printed_evolutions(namespace: &Namespace) -> Vec<Value> {
    vec![
        json!({
            "id": namespace.urn_text("Vehicle", "A8311"),
            "type": namespace.suffixed("Vehicle"),
            "scope": {"type": "Property", "values": [[["/Madrid/Centro", "/Company123/UnitA"], "2018-08-01T12:10:00Z"]]},
            "speed": {"type": "Property", "values": [
                [40, "2018-08-01T12:12:00Z"],
                [60, "2018-08-01T12:14:00Z"],
                [50, "2018-08-01T12:16:00Z"],
            ]},
        }),
        json!({
            "id": namespace.urn_text("Vehicle", "B9211"),
            "type": namespace.suffixed("Vehicle"),
            "scope": {"type": "Property", "values": [["/Madrid/Centro", "2018-08-01T11:00:00Z"]]},
            "speed": {"type": "Property", "values": [
                [30, "2018-08-01T12:03:00Z"],
                [60, "2018-08-01T12:05:00Z"],
                [50, "2018-08-01T12:07:00Z"],
            ]},
        }),
    ]
}

/// Pins: Query Temporal Evolution of Entities with scopeQ answers the scope and speed evolution of both Vehicles in
/// that scope as C.5.16 prints, selected by attrs=speed,scope and again by pick
/// Clause: C.5.16, 4.18, 4.5.9, 5.7.4
#[gtest]
#[tokio::test]
async fn c_5_16_0001_a_temporal_scope_query_answers_the_scope_and_speed_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    record(
        &broker,
        &namespace,
        "B9211",
        &["/Madrid/Centro"],
        "2018-08-01T11:00:00Z",
        [(30, "2018-08-01T12:03:00Z"), (60, "2018-08-01T12:05:00Z"), (50, "2018-08-01T12:07:00Z")],
    )
    .await?;
    record(
        &broker,
        &namespace,
        "A8311",
        &["/Madrid/Centro", "/Company123/UnitA"],
        "2018-08-01T12:10:00Z",
        [(40, "2018-08-01T12:12:00Z"), (60, "2018-08-01T12:14:00Z"), (50, "2018-08-01T12:16:00Z")],
    )
    .await?;
    let window = || -> Result<_> { Ok(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?)) };
    let by_attrs = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query_values(window()?)
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .attrs(vec!["speed".parse()?, "scope".parse()?])
            .scope_q(scope_query::scope("/Madrid/Centro"))
            .send()
            .await,
    )?;
    expect_flat(by_attrs, printed_evolutions(&namespace))?;
    let by_pick = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query_values(window()?)
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .pick(projection::attributes(["id", "type", "speed", "scope"]).build()?)
            .scope_q(scope_query::scope("/Madrid/Centro"))
            .send()
            .await,
    )?;
    expect_flat(by_pick, printed_evolutions(&namespace))?;
    served.stop();
    Ok(())
}
