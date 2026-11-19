//! A body member written with a token it does not take is named with the tokens it takes.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, RegisterContextSource},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with `notificationTrigger` `["bogus"]` and Create Context Source Registration with
/// `mode` `bogus` answer 400 `BadRequestData` whose detail names the member, the value sent and every token the
/// member takes, with no backtick, no "variant" and no line or column of the body, and neither is stored
/// Clause: 5.2.9, 5.2.12, 5.5.4, 5.8.1.4, 5.9.2.4
/// Recorded: clause 5.5.4 fixes the error type but not the wording; Andromeda words the refusal in the members and
/// values of the request
#[gtest]
#[tokio::test]
async fn o_cmn_val_9040_a_body_member_written_with_a_token_it_does_not_take_is_named_with_the_tokens_it_takes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("trigger")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (unknown notification trigger), so `notificationTrigger` is written as text.
    let triggers: RawJson = r#"["bogus"]"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("notificationTrigger", &triggers);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(
            "the request body member 'notificationTrigger[0]' does not decode: 'bogus' is not one of entityCreated, entityUpdated, entityDeleted, attributeCreated, attributeUpdated, attributeDeleted"
        )
    );
    let missing_subscription = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&missing_subscription, answered(StatusCode::NOT_FOUND));
    let registration_id = namespace.registration_id("mode")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .build()?;
    // The model refuses it (unknown mode), so `mode` is written as text.
    let document = UncheckedDocument::try_from(registration.render())?.set_text("mode", "bogus");
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("the request body member 'mode' does not decode: 'bogus' is not one of inclusive, exclusive, redirect, auxiliary")
    );
    let missing_registration = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&missing_registration, answered(StatusCode::NOT_FOUND));
    Ok(())
}
