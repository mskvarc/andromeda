//! The 207 of an update is an update result.

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
    model::{Entity, EntityFragment},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Update Entity Attributes of speed matching an inclusive registration whose source answers 500 answers 207
/// with an `UpdateResult` whose `notUpdated` names speed
/// Clause: 5.6.2.4, 6.3.17, 5.2.18, 5.2.19, 6.6.3.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7221_the_207_of_an_update_is_an_update_result() -> Result<()> {
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
        .operations([DistributedOperation::UpdateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let fragment = EntityFragment::builder().property("speed", 50).build()?;
    let received = broker.with_responses().entities().update_attributes(&vehicle_id, &fragment).send().await?;
    let report = received.outcome.partial()?;
    // The report reads an absent `updated` as an empty one, so its presence is read as received.
    expect_that!(&received.body, json_exists("$.updated"));
    expect_that!(&report.not_updated, len(eq(1)));
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    Ok(())
}
