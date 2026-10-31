//! Purge is forwarded only to the registrations whose location may hold a geoquery match.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{
        RawJson,
        geometry::{Position, Shape},
    },
    query::geoquery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities by type with a near geoquery under redirect registrations advertising purgeEntity, one located inside the area, one far from it and one without a location, answers 204 and forwards a purge to the first and the third sources only
/// Clause: 5.6.21.4, 4.10, 4.3.6.1, 5.10.2.4
/// Tenant: private (it counts every request its context sources receive, and a purge forwarded to every
/// registration of the tenant reaches the sources of other tests)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9316_purge_is_forwarded_only_to_the_registrations_whose_location_may_hold_a_geoquery_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_a = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
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
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
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
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::DELETE, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/b/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/c/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .geo_q(geoquery::near_max(3000, Shape::Point(Position::try_new(-3.7038, 40.4168)?)))
        .send()
        .await?
        .done()?;
    let asked_a = source
        .received_matching(&SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source
        .received_matching(&SourceRequest::any_matching("/b/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_b, len(eq(0)));
    let asked_c = source
        .received_matching(&SourceRequest::any_matching("/c/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_c, len(eq(1)));
    Ok(())
}
