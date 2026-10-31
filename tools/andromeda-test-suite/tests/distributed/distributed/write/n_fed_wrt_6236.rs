//! Replace of an attribute is forwarded only when the type selector names the registered type.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Attribute, AttributeName, ErrorType, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Attribute of speed with type naming another type than the registered one answers 404
/// `ResourceNotFound` and forwards nothing; with the registered type it answers 204 and forwards one put
/// Clause: 5.6.19.4, 5.12, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6236_replace_of_an_attribute_is_forwarded_only_when_the_type_selector_names_the_registered_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::ReplaceAttrs, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let speed_path = format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed");
    source.answer(Method::PUT, &speed_path, source_answer::status(StatusCode::NO_CONTENT)).await?;
    let speed = "speed".parse::<AttributeName>()?;
    let attribute = Attribute::from(Property::builder(99).build()?);
    let answer = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_id, &speed, &attribute)
        .entity_type(&building_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let forwarded = source.received_on(Method::PUT, &speed_path).await?;
    expect_that!(forwarded, len(eq(0)));
    broker
        .entities()
        .replace_attribute(&vehicle_id, &speed, &attribute)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let forwarded_2 = source.received_on(Method::PUT, &speed_path).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
