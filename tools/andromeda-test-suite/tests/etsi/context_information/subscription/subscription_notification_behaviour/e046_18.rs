//! ETSI `046_18`: a notification omits the system generated attributes of the entity when `sysAttrs` is `false` (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{first_notification_with_sys_attrs, teardown},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    subscription::{SubscriptionId, SystemAttributeReporting},
};
use serde_json::Value;

/// Pins: The system generated attributes are not included in the response payload body of a notification if sysAttrs parameter
/// is set to false.
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: Create Subscription requires the 201 answer, where the Hurl request accepts any answer; a refused subscription leaves
/// no notification to wait for, so the case fails either way
#[gtest]
#[tokio::test]
async fn e046_18_01_check_that_the_response_payload_body_does_not_contain_the_system_generated_attributes_if_sys_attrs_parameter_is_set_to_false() -> Result<()>
{
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
    let (notification, _subscription) = first_notification_with_sys_attrs(&as_string, &as_list, &sink, &building, &id, SystemAttributeReporting::Omit).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/data/0"), some(predicate(Value::is_object)));
    expect_that!(body.pointer("/data/0/createdAt"), none());
    expect_that!(body.pointer("/data/0/modifiedAt"), none());

    teardown(&broker, &id, &building).await;
    Ok(())
}
