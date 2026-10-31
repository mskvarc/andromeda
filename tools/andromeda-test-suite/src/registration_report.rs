//! The context source notifications a test of racing registration changes reads (clause 5.11.7).
//!
//! Each Context Source Registration Subscription of such a test notifies an endpoint of its own of one
//! [`NotificationSink`], numbered from 0, so the notifications of one endpoint are those of one subscription, in
//! arrival order. Proving that a subscription was notified exactly once takes a wait as long as a duplicate could
//! take: the [`QUIET_WINDOW`], waited once for every endpoint of the test together, after which each endpoint's count
//! is read.

use crate::notification_sink::NotificationSink;
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityTypeName, Input, NgsiLdMediaType, Output},
    registration::{CSourceNotification, CSourceRegistration, RegistrationId},
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription, SubscriptionId},
};
use std::{ops::Range, time::Duration};
use tokio::time::sleep;

/// How long a duplicate notification is waited for once every awaited notification has arrived: past the delivery
/// latency of a broker that other tests keep busy.
pub const QUIET_WINDOW: Duration = Duration::from_secs(5);

/// The Context Source Registration Subscription `id` selecting the registrations of `entity_types`, notifying
/// endpoint `endpoint` of `sink` in JSON.
///
/// # Errors
///
/// Returns the failure of building the endpoint, the notification parameters or the subscription.
pub fn registration_subscription(
    id: SubscriptionId,
    entity_types: &[&EntityTypeName],
    sink: &NotificationSink,
    endpoint: usize,
) -> Result<Subscription<Input, RegistrationSubscription>> {
    Ok(Subscription::registration_builder()
        .id(id)
        .entities(entity_types.iter().map(|entity_type| entity_type.as_str()))
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?)
}

/// The `k`-th notification (1 for the first) endpoint `endpoint` of `sink` receives, read as a context source
/// notification, waiting up to the delivery deadline for it.
///
/// # Errors
///
/// Returns the failure of the wait, or of reading the body as a context source notification.
pub async fn report(sink: &NotificationSink, endpoint: usize, k: usize) -> Result<CSourceNotification> {
    Ok(sink.wait_for(endpoint, k).await?.registrations()?)
}

/// The ids of the registrations `notification` carries, in its order.
#[must_use]
pub fn reported_ids(notification: &CSourceNotification) -> Vec<&RegistrationId> {
    notification.data().iter().map(CSourceRegistration::<Output>::id).collect()
}

/// How many notifications each endpoint of `endpoints` of `sink` holds once the [`QUIET_WINDOW`] has passed, in the
/// order of `endpoints`.
///
/// # Errors
///
/// Returns the failure of reading what an endpoint received.
pub async fn held_after_quiet_window(sink: &NotificationSink, endpoints: Range<usize>) -> Result<Vec<usize>> {
    sleep(QUIET_WINDOW).await;
    join_all(endpoints.map(|endpoint| sink.received(endpoint)))
        .await
        .into_iter()
        .map(|received| Ok(received?.len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        notification_sink::NotificationSink,
        registration_report::{registration_subscription, reported_ids},
    };
    use googletest::prelude::*;
    use ngsi_ld::registration::CSourceNotification;

    #[gtest]
    fn the_reported_ids_are_those_of_the_carried_registrations_in_their_order() -> Result<()> {
        let notification = CSourceNotification::parse(concat!(
            r#"{"id":"urn:ngsi-ld:Notification:1","type":"ContextSourceNotification","subscriptionId":"urn:ngsi-ld:Subscription:1","#,
            r#""notifiedAt":"2024-01-01T00:00:00Z","triggerReason":"newlyMatching","data":["#,
            r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:2","type":"ContextSourceRegistration","#,
            r#""information":[{"entities":[{"type":"Bus"}]}],"endpoint":"http://source.test/"},"#,
            r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:1","type":"ContextSourceRegistration","#,
            r#""information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://source.test/"}]}"#
        ))?;
        verify_that!(
            reported_ids(&notification).iter().map(ToString::to_string).collect::<Vec<_>>(),
            elements_are![eq("urn:ngsi-ld:ContextSourceRegistration:2"), eq("urn:ngsi-ld:ContextSourceRegistration:1")]
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_registration_subscription_selects_every_type_it_is_given() -> Result<()> {
        let namespace = Namespace::fresh();
        let sink = NotificationSink::start().await?;
        let vehicle_type = namespace.entity_type("Vehicle")?;
        let bus_type = namespace.entity_type("Bus")?;
        let subscription = registration_subscription(namespace.subscription_id(1)?, &[&vehicle_type, &bus_type], &sink, 0)?;
        verify_that!(subscription.entities(), len(eq(2)))
    }
}
