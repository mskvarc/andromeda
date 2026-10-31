//! The 207 of a partial update is an update result.

use andromeda_test_suite::{
    body_json::json_exists,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    report_matcher::not_updated,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, UpdateResult, operation::PartialAttributeUpdate},
    model::{Attribute, AttributeName, Change, Entity, Lossless, Property, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Partial Attribute Update of speed matching an inclusive registration whose source answers 500 answers 207
/// with an `UpdateResult` whose `notUpdated` names speed
/// Clause: 5.6.4.4, 6.3.17, 5.2.18, 5.2.19, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7222_the_207_of_a_partial_update_is_an_update_result() -> Result<()> {
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
        .operations([DistributedOperation::UpdateAttrs])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let speed = "speed".parse::<AttributeName>()?;
    let change = Change::from(Attribute::from(Property::builder(60).build()?).into_patch());
    let operation = PartialAttributeUpdate::builder()
        .id(&vehicle_id)
        .name(&speed)
        .attribute(&change)
        .form(Lossless::Normalized)
        .build();
    // The fragment is a value without a type, which the model writes only as a bare value, so the body is written as
    // text.
    let value: RawJson = "60".parse()?;
    let document = UncheckedDocument::empty().set_member("value", &value);
    let request = broker.unchecked(&operation)?.document(&document);
    let response = broker.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::MULTI_STATUS));
    // The report reads an absent `updated` as an empty one, so its presence is read as received.
    expect_that!(response.body(), json_exists("$.updated"));
    let report = serde_json::from_slice::<UpdateResult>(response.body())?;
    expect_that!(&report.not_updated, len(eq(1)));
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    Ok(())
}
