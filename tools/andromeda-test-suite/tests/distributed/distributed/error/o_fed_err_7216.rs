//! A subscription copy after a timed out read is placed at a source whose registration sets no cooldown.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RegisterContextSource,
    },
    model::{Entity, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;

/// Pins: Create Subscription after a Retrieve Entity whose inclusive source answered after the registration timeout of 3000 ms, under a registration with no cooldown that offers createSubscription, places one copy at the source
/// Clause: 6.3.17, 5.2.34, 5.8.1.4
/// Recorded: clause 6.3.17 lets a broker optionally decline further requests to a failed endpoint during its cooldown; Andromeda holds a failed source back only when its registration sets management.cooldown
#[gtest]
#[tokio::test]
async fn o_fed_err_7216_a_subscription_copy_after_a_timed_out_read_is_placed_at_a_source_whose_registration_sets_no_cooldown() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::CreateSubscription,
            DistributedOperation::UpdateSubscription,
            DistributedOperation::DeleteSubscription,
        ])
        .build()?;
    // The model refuses it (a registration management), so `management` is written as text.
    let management: RawJson = r#"{"timeout": 3000}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity = Entity::output_builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::entity(&source_entity).set_delay(Duration::from_millis(10_000)),
        )
        .await?;
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer_when(
            SourceRequest::matching(Method::DELETE, "/a/ngsi-ld/v1/subscriptions/.*"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let placed = source.received_on(Method::POST, "/a/ngsi-ld/v1/subscriptions").await?;
    expect_that!(placed, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
