//! ETSI `047_13`: a Context Source Registration Subscription without `watchedAttributes` is notified of a registration
//! whose information matches its `entities` (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::notified_of_building_source,
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::subscription::{Activation, SubscriptionId};

/// Pins: Check if a context source registrations subscription does not define watchedAttributes member, a
/// `CsourceNotification` will be triggered from context source registrations with information member matching all
/// attributes of the described entities
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_13_01_receive_csource_notification_for_matching_context_source_registrations_on_any_watched_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let subscription = building_registration_subscription(Some(id), &sink.endpoint(1)?, Activation::Unspecified, None)?;
    notified_of_building_source(&namespace, &sink, &subscription).await
}
