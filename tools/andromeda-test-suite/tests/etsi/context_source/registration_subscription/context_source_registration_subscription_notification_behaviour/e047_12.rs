//! ETSI `047_12`: a Context Source Registration Subscription with `entities` and `watchedAttributes` is notified of a
//! registration whose information matches both (clause 5.11.7).

use crate::context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::notified_of_building_source;
use andromeda_test_suite::{namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check if a context source registrations subscription defines entities member and watchedAttributes member,
/// a `CsourceNotification` will be triggered from context source registrations with information member matching the
/// described "entities" and "attributes"
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_12_01_receive_csource_notification_for_matching_context_source_registrations_on_watched_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let subscription = Subscription::registration_builder()
        .id(id)
        .watched_attributes(["name", "subCategory"])
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?.as_str()).build()?).build()?)
        .build()?;
    notified_of_building_source(&namespace, &sink, &subscription).await
}
