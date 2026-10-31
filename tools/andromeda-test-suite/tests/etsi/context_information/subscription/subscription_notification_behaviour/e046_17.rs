//! A notification carries the system generated attributes of the entity when `sysAttrs` is `true` (clause 5.8.6).

use crate::{
    building_payloads::building_simple_attributes,
    context_information::subscription::subscription_notification_behaviour::notification_case::{first_notification_with_sys_attrs, teardown},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    subscription::{SubscriptionId, SystemAttributeReporting},
};

/// Pins: The system generated attributes are included in the response payload body of a notification if sysAttrs parameter is
/// set to true.
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_17_01_check_that_the_response_payload_body_contains_the_system_generated_attributes_if_sys_attrs_parameter_is_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let (notification, _subscription) =
        first_notification_with_sys_attrs(&as_string, &as_list, &sink, &building, &id, SystemAttributeReporting::Include).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/data/0/createdAt"), some(anything()));
    expect_that!(body.pointer("/data/0/modifiedAt"), some(anything()));

    teardown(&broker, &id, &building).await;
    Ok(())
}
