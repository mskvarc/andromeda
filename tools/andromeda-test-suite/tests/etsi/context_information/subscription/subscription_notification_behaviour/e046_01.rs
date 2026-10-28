//! ETSI `046_01`: a subscription notifies only while its status is active (clause 5.8.6).

use crate::suite_payloads::building_simple_attributes;
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    robot_literal,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::send_to,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{CreateEntity, CreateSubscription, UpdateAttributes},
    },
    model::{EntityFragment, NgsiLdMediaType, Property, Representation},
    query::EntitySelector,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Pins: Check that a notification is only sent if and only if the status is active
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three and
/// then requires the body to be JSON, which is stricter only where an escape leaves the keyword reading, and accepting,
/// a body other than the one sent
#[gtest]
#[tokio::test]
async fn e046_01_01_check_that_a_notification_is_only_sent_if_status_is_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    let created = send_to(&as_string, &CreateEntity::builder().entity(&building).build(), "entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    sleep(Duration::from_secs(1)).await;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = Cleanup::of(&as_list).subscription_of(&subscription);
    let subscribed = send_to(&as_list, &CreateSubscription { subscription: &subscription }, "subscriptions/").await;
    verify_that!(&subscribed, answered(StatusCode::CREATED))?;

    let fragment = EntityFragment::builder()
        .property(
            "airQualityLevel",
            Property::builder(5).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .build()?;
    let update = UpdateAttributes::builder().id(&building.id).fragment(&fragment).build();
    send_to(&as_list, &update, &format!("entities/{}/attrs/", building.id)).await.any_answer();

    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(body.pointer("/data/0/airQualityLevel/value"), some(eq(&json!(5))));

    broker.subscriptions().delete(&id).send().await.any_answer();
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
