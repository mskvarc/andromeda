//! A delete under an exclusive registration without delete entity is a partial success.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    problem::answered,
    report_matcher::batch_error,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Relationship},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Entity of an Entity held locally whose speed is registered exclusively with no operations member
/// answers 207 with one error of status 409, forwards nothing and removes the local Entity
/// Clause: 5.6.6.4, 4.3.6.1, 4.20
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0004_a_delete_under_an_exclusive_registration_without_delete_entity_is_a_partial_success() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(
            SourceRequest::matching(Method::DELETE, "/a/ngsi-ld/v1/entities/[^/]+"),
            source_answer::status(StatusCode::MULTI_STATUS),
        )
        .await?;
    let report = broker.entities().delete(&vehicle_id).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().status(StatusCode::CONFLICT)));
    let forwarded_request = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
