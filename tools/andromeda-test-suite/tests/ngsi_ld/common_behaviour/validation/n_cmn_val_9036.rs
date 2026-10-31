//! The purge parameters are undefined on Query Entities.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    entity_matcher::lacks_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedRequest, headers::Accept},
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities with drop, keep or `deleteAll`, alone or beside a type selector, answers 400
/// `InvalidRequest`; drop and keep shape a Purge Entities and `deleteAll` a Delete Attribute, so Table 6.4.3.2-1
/// defines none of them and none counts as a narrowing input
/// Clause: 6.3.20, 6.4.3.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9036_the_purge_parameters_are_undefined_on_query_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "purge-parameter")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Query Entities defines none of these parameters, which the client never writes, so each read goes out as written.
    let targets = [
        "entities?drop=speed".to_owned(),
        "entities?keep=speed".to_owned(),
        "entities?deleteAll=true".to_owned(),
        format!("entities?type=Vehicle_{namespace}&drop=speed"),
        format!("entities?type=Vehicle_{namespace}&keep=speed"),
        format!("entities?type=Vehicle_{namespace}&deleteAll=true"),
    ];
    for target in &targets {
        let answer = broker.send_unchecked(ld_json_read(&broker, target)?).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    }
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .drop(vec!["speed".parse::<AttributeName>()?])
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), lacks_attribute("speed"));
    Ok(())
}

/// A GET to `target` asking for `application/ld+json`.
fn ld_json_read(broker: &BrokerClient, target: &str) -> Result<UncheckedRequest> {
    Ok(request_to(broker, Method::GET, target)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json")))
}
