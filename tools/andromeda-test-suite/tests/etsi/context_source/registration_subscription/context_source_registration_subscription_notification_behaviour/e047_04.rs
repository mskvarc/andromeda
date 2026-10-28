//! ETSI `047_04`: a csource notification has the structure of clause 5.3.2 (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::notified_of_building_source,
    suite_payloads::building_registration_subscription,
};
use andromeda_test_suite::{namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::subscription::{Activation, SubscriptionId};

/// Pins: The structure of the csource notification message shall be as mandated by clause 5.3.2
/// Clause: 5.11.7
/// Corrected: `fix(047-notification)`: register a matching context source and consume the initial notification (5.12)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_04_01_receive_csource_notification_with_compliant_structure() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let subscription = building_registration_subscription(Some(id), &sink.endpoint(1)?, Activation::Unspecified, None)?;
    notified_of_building_source(&namespace, &sink, &subscription).await
}
