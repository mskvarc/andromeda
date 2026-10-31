//! A temporal map asks only the registrations whose location may hold a geoquery match.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities by type with a near geoquery under inclusive registrations advertising createEntityMapQueryTemporal, one located inside the area, one far from it and one without a location, answers 201 and asks the first and the third sources only
/// Clause: 5.14.5.4, 4.10, 4.3.6.1, 5.10.2.4
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qte_9315_a_temporal_map_asks_only_the_registrations_whose_location_may_hold_a_geoquery_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_a = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_a: RawJson = r#"{"type": "Point", "coordinates": [-3.7038, 40.4168]}"#.parse()?;
    let document_a = UncheckedDocument::try_from(registration_a.render())?.set_member("location", &location_a);
    let request_a = broker
        .unchecked(&RegisterContextSource { registration: &registration_a })?
        .document(&document_a);
    let answer_a = broker.send_unchecked(request_a).await;
    expect_that!(&answer_a, answered(StatusCode::CREATED));
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location_b: RawJson = r#"{"type": "Point", "coordinates": [13.405, 52.52]}"#.parse()?;
    let document_b = UncheckedDocument::try_from(registration_b.render())?.set_member("location", &location_b);
    let request_b = broker
        .unchecked(&RegisterContextSource { registration: &registration_b })?
        .document(&document_b);
    let answer_b = broker.send_unchecked(request_b).await;
    expect_that!(&answer_b, answered(StatusCode::CREATED));
    let registration_c = CSourceRegistration::builder(source.endpoint("c")?)
        .id(namespace.registration_id(3)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/c/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .geo_q(geoquery::near_max(3000, Shape::Point(Position::try_new(-3.7038, 40.4168)?)))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(0)));
    let asked_c = source.received_matching(&SourceRequest::any_matching("/c/.*")).await?;
    expect_that!(asked_c, len(eq(1)));
    Ok(())
}
