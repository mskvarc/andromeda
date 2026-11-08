//! A list relationship written for a version before 1.8 keeps every target.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::{Entity, ListRelationship, Lossless, NgsiLdMediaType, OneOrMany, Value},
};

/// Pins: Retrieve Entity with Prefer ngsi-ld=1.7 answers 203 with the version echoed and writes a `ListRelationship`
/// of three targets as a Relationship whose `object` names all three in list order, in the normalized and concise
/// representations, and as the array of the three targets in the simplified one
/// Clause: 4.3.6.8, 4.5.3, 4.5.22, 6.3.6
#[gtest]
#[tokio::test]
async fn n_ent_ret_9306_a_list_relationship_written_for_a_version_before_1_8_keeps_every_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let route_id = namespace.urn("Route", 1)?;
    let stops = [3, 1, 2]
        .map(|number| namespace.urn("Stop", number))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let route = Entity::builder()
        .id(route_id.clone())
        .entity_type(namespace.entity_type("Route")?)
        .list_relationship("stops", ListRelationship::builder(stops.clone()))
        .build()?;
    broker.entities().create(&route).send().await?.done()?;
    let version: SpecVersion = "1.7".parse()?;
    let versioned = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_prefer(Prefer::builder().version(version).build()?);
    // Table 4.3.6.8-1: a target before 1.8 has no ListRelationship, and Table 4.3.6.8-3 types a Relationship's
    // `object` as String or String[], so every target is kept in list order.
    let every_stop = OneOrMany::try_from(stops.clone())?;
    for format in [Lossless::Normalized, Lossless::Concise] {
        let retrieved = versioned
            .entities()
            .retrieve(&route_id)
            .form(format)
            .send()
            .await?
            .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
        expect_that!(retrieved.preference_applied(), some(eq(&version)), "{format:?}");
        expect_that!(
            retrieved.body(),
            has_attribute("stops", relationship().object(every_stop.clone())),
            "{format:?}"
        );
    }
    let simplified = versioned
        .entities()
        .retrieve_simplified(&route_id)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(simplified.preference_applied(), some(eq(&version)));
    let targets = stops.iter().map(|stop| Value::from(stop.to_string())).collect();
    expect_that!(simplified.body(), has_value("stops", Value::Array(targets)));
    broker.entities().delete(&route_id).send().await?.done()?;
    Ok(())
}
