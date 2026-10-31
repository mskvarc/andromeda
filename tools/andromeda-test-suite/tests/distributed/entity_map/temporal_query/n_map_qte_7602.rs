//! A posted temporal query creates a map of local and peer evolutions.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Upserted,
    wire::header_value,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    model::{Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, RegistrationInfo, RegistrationMode},
};
use std::str;

/// Pins: POST `temporal/entityMaps` on broker A with a Query by type and a `temporalQ`, over a local evolution and
/// one broker B serves through an inclusive registration, answers 201 with an `NGSILD-EntityMap` header naming an
/// `EntityMap` of both
/// Clause: 5.14.5.4, 6.34
#[gtest]
#[tokio::test]
async fn n_map_qte_7602_a_posted_temporal_query_creates_a_map_of_local_and_peer_evolutions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(2).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal, DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The client builds a temporal map from query parameters; the posted Query document is written as text.
    let body = format!(
        r#"{{
    "type": "Query",
    "entities": [
        {{
            "type": "Vehicle_{namespace}"
        }}
    ],
    "temporalQ": {{
        "timerel": "before",
        "timeAt": "2022-01-01T00:00:00Z"
    }}
}}"#
    );
    let request = json_request_to(&broker, Method::POST, "temporal/entityMaps", &body)?;
    let response = broker.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::CREATED));
    let map = EntityMap::parse(str::from_utf8(response.body())?)?;
    expect_that!(
        header_value(response.headers(), &HeaderName::from_static("ngsild-entitymap")),
        some(eq(map.id.to_string().as_str()))
    );
    expect_that!(&map.entity_map, len(eq(2)));
    Ok(())
}
