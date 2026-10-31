//! The 207 of an attribute replace is an update result.

use andromeda_test_suite::{
    body_json::json_exists,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::not_updated,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Attribute, AttributeName, Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Replace Entity Attribute of speed matching an inclusive registration whose source answers 500 answers 207
/// with an `UpdateResult` whose `notUpdated` names speed
/// Clause: 5.6.19.4, 6.3.17, 5.2.18, 5.2.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7224_the_207_of_an_attribute_replace_is_an_update_result() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .property("brandName", "x")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::ReplaceAttrs])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(
            Method::PUT,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let attribute = Attribute::from(Property::builder(77).build()?);
    let received = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await?;
    let report = received.outcome.partial()?;
    // The report reads an absent `updated` as an empty one, so its presence is read as received.
    expect_that!(&received.body, json_exists("$.updated"));
    expect_that!(&report.not_updated, len(eq(1)));
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    Ok(())
}
