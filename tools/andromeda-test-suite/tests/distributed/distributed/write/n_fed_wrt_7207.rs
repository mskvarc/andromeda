//! A partial update whose inclusive source answers 500 is a 207 and the attribute is updated locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::reported,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, UpdateResult, headers::Accept, operation::PartialAttributeUpdate},
    model::{Attribute, AttributeName, Change, Entity, Lossless, Property, RawJson},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Partial Attribute Update of speed matching an inclusive registration whose source answers 500 answers 207
/// and the attribute is updated locally
/// Clause: 5.6.4.4, 6.3.17, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7207_a_partial_update_whose_inclusive_source_answers_500_is_a_207_and_the_attribute_is_updated_locally() -> Result<()> {
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
    let speed_path = format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed");
    source
        .answer(Method::PATCH, &speed_path, source_answer::status(StatusCode::INTERNAL_SERVER_ERROR))
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
    let answer = broker.send_unchecked(request).await;
    reported::<UpdateResult, _>(answer)?;
    let forwarded = source.received_on(Method::PATCH, &speed_path).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(60)));
    Ok(())
}
