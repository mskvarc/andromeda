//! ETSI `047_02`: a Context Source Registration Subscription without a `timeInterval` notifies initially on
//! subscription and on a change of a matching registration (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_csource_notification,
        registered,
        teardown,
    },
    suite_keywords::{create_registration_subscription, csource_endpoint, ld_json_body_client},
    suite_payloads::{building_registration, building_registration_subscription},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    registration::RegistrationFragment,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that if the created context source registration subscription does not define a timeInterval member,
/// a cSourceNotification, with the appropriate trigger reason in the "triggerReason" member, will be sent initially on
/// subscription and whenever there is a change of a matching Context Source Registration
/// Clause: 5.11.7
/// Corrected: `fix(047-notification)`: register a matching context source and consume the initial notification (5.12)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_02_01_receive_csource_notification_initially_and_on_a_change_of_a_matching_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = building_registration(&namespace, 1, &endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered(&broker, &vocabulary, &registration).await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;

    let first = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&first.body)?;
    expect_csource_notification(&body, &id, registration_id, "newlyMatching");

    let new_endpoint = csource_endpoint(&namespace, "my.new.csource.org:1026").await?;
    let fragment = RegistrationFragment::builder().endpoint(new_endpoint.as_str()).build()?;
    broker.registrations().update(registration_id, &fragment).send().await.any_answer();

    let second = sink.wait_for(1, 2).await?;
    let body = robot_literal::read(&second.body)?;
    expect_csource_notification(&body, &id, registration_id, "updated");

    teardown(&broker, registration_id, &id).await
}
