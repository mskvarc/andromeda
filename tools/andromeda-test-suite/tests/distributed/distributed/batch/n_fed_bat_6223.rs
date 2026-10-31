//! Update reports local successes and misses beside an auxiliary registration.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Update of a locally held Vehicle and of an unknown one under an auxiliary registration
/// answers 207 with the held one in success and a `ResourceNotFound` error for the unknown one, and forwards
/// nothing
/// Clause: 5.6.9.4, 4.3.6.5
#[gtest]
#[tokio::test]
async fn n_fed_bat_6223_update_reports_local_successes_and_misses_beside_an_auxiliary_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragments = [
        EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
            .property("speed", 99)
            .build()?,
        EntityFragment::for_entity(vehicle_2_id.clone(), vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    let report = broker.entities().batch_update(&fragments).send().await?.partial()?;
    expect_that!(&report.success, contains(eq(&vehicle_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_2_id).status(StatusCode::NOT_FOUND)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(99)));
    let forwarded_request = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
