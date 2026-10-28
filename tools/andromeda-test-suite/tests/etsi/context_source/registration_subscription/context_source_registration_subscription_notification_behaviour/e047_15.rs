//! ETSI `047_15`: a Context Source Registration Subscription with a `geoQ` that names no `geoproperty` is matched
//! against the `location` of a registration (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        located_building_registration,
        notified_of_source,
    },
    suite_payloads::point,
};
use andromeda_test_suite::{namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    query::{EntitySelector, geoquery},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check if a context source registrations subscription does not define a geoproperty in the geoQ member, a
/// `CsourceNotification` will be triggered from matching context source registrations with a matching location member
/// Clause: 5.11.7
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
#[gtest]
#[tokio::test]
async fn e047_15_01_receive_csource_notification_for_matching_context_source_registrations_on_location_as_default() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let subscription = Subscription::registration_builder()
        .id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?.as_str()).build()?).build()?)
        .geo_q(geoquery::near_max(2000, point(-8.52, 41.2)?).build()?)
        .build()?;
    notified_of_source(&namespace, &sink, &["Building"], &subscription, |endpoint| {
        located_building_registration(&namespace, endpoint)
    })
    .await
}
