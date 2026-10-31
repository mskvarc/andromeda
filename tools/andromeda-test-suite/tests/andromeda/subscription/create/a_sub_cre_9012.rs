//! A subscription q holding an undefined construct is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ErrorType, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Create Subscription with a q pattern holding a negated class name, a scoped inline option, \b or a Unicode
/// property class answers 400 `BadRequestData`
/// Clause: 4.9, 5.8.1.4
/// Recorded: IEEE 1003.2 leaves a backslash before an ordinary character, an inline option, a repeated duplication
/// symbol, an undefined class name and a range outside the POSIX locale undefined or unspecified; Andromeda refuses
/// each with `BadRequestData` so that the stored-query and the subscription evaluators never read one pattern two
/// ways
#[gtest]
#[tokio::test]
async fn a_sub_cre_9012_a_subscription_q_holding_an_undefined_construct_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_pattern_refused_0_id = namespace.subscription_id("pattern-refused-0")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_pattern_refused_1_id = namespace.subscription_id("pattern-refused-1")?;
    let subscription_pattern_refused_2_id = namespace.subscription_id("pattern-refused-2")?;
    let subscription_pattern_refused_3_id = namespace.subscription_id("pattern-refused-3")?;
    let subscription_pattern_refused_0 = Subscription::builder()
        .id(subscription_pattern_refused_0_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"[[:^alpha:]]\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("pattern-refused-0")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer = broker.with_responses().subscriptions().create(&subscription_pattern_refused_0).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_pattern_refused_1 = Subscription::builder()
        .id(subscription_pattern_refused_1_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"(?m:Merc)\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("pattern-refused-1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer_2 = broker.with_responses().subscriptions().create(&subscription_pattern_refused_1).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_pattern_refused_2 = Subscription::builder()
        .id(subscription_pattern_refused_2_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"\\bMer\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("pattern-refused-2")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer_3 = broker.with_responses().subscriptions().create(&subscription_pattern_refused_2).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_pattern_refused_3 = Subscription::builder()
        .id(subscription_pattern_refused_3_id)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"\\p{L}\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("pattern-refused-3")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer_4 = broker.with_responses().subscriptions().create(&subscription_pattern_refused_3).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
