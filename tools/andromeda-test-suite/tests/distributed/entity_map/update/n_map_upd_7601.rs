//! An update changes the expiry and ignores output only members.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    received_status::ExpectedStatus,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, EntitySource, RegistrationId, RegistrationInfo, RegistrationMode},
};

/// Pins: Update `EntityMap` on broker A with `expiresAt` 2999-01-01T00:00:00Z, an `entityMap` and `linkedMaps`
/// answers 204, and the map reads back with that `expiresAt` and its own two entries only; an unknown map answers
/// 404 `ResourceNotFound`
/// Clause: 5.14.2.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_upd_7601_an_update_changes_the_expiry_and_ignores_output_only_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let created = broker
        .with_heads()
        .with_accept(Accept::any())
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    // An update document with the output-only members `entityMap` and `linkedMaps` is no fragment the client writes.
    let body = format!(
        r#"{{
    "expiresAt": "2999-01-01T00:00:00Z",
    "entityMap": {{
        "urn:ngsi-ld:Vehicle:{namespace}-fake": [
            "@none"
        ]
    }},
    "linkedMaps": {{
        "urn:ngsi-ld:ContextSourceRegistration:{namespace}-fake": "urn:ngsi-ld:EntityMap:{namespace}-fake"
    }}
}}"#
    );
    let request = json_request_to(&broker, Method::PATCH, &format!("entityMaps/{map_id}"), &body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let map = broker.with_accept(Accept::any()).entity_maps().retrieve(map_id).send().await?;
    let expires_at: DateTime = "2999-01-01T00:00:00Z".parse()?;
    expect_that!(&map.expires_at, eq(&expires_at));
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(
        map.entity_map.get(&vehicle_id).and_then(|sources| sources.first()),
        some(eq(&EntitySource::Local))
    );
    expect_that!(
        map.entity_map.get(&vehicle_2_id).and_then(|sources| sources.first()),
        some(eq(&EntitySource::Registration(registration_id)))
    );
    let fake_registration_id = format!("urn:ngsi-ld:ContextSourceRegistration:{namespace}-fake").parse::<RegistrationId>()?;
    expect_that!(map.linked_maps.get(&fake_registration_id), none());
    let unknown_id = format!("urn:ngsi-ld:EntityMap:{namespace}-unknown").parse::<EntityMapId>()?;
    let unknown = broker.with_responses().entity_maps().update(&unknown_id, &expires_at).send().await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
