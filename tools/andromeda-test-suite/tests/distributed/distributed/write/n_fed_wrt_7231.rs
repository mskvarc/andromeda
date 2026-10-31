//! The 207 of a create names the created entity in its location header.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, partial_outcome::PartialSuccess, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Location,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Create Entity matching an inclusive registration whose source answers 500 answers 207 with a Location
/// header holding the relative path of the created entity
/// Clause: 6.4.3.1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7231_the_207_of_a_create_names_the_created_entity_in_its_location_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::INTERNAL_SERVER_ERROR))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    let received = broker.with_responses().entities().create(&vehicle).send().await?;
    received.outcome.partial()?;
    expect_that!(received.status, eq(StatusCode::MULTI_STATUS));
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(eq(format!("/ngsi-ld/v1/entities/{vehicle_id}").as_str()))
    );
    Ok(())
}
