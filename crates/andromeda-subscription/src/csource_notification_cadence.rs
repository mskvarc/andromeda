//! What a Context Source Registration Subscription is owed, and when
//! (ETSI GS CIM 009 clauses 5.11.2.4 and 5.11.7).
//!
//! Clause 5.11.2.4 states the two cadences in two bullets, and they differ in
//! more than their timing. A subscription that defines a `timeInterval` is
//! owed a notification "periodically, initially on subscription and when the
//! time interval (in seconds) specified in such value field is reached,
//! independent of any changes to the set of Context Source registrations": the
//! schedule alone decides, so the notification goes out carrying whatever
//! currently matches, empty set included. A subscription without one is owed a
//! notification "initially on subscription and whenever there is a change of a
//! matching Context Source Registration (creation, update, deletion) ...
//! informing about this change": what is notified is a change to a matching
//! registration, so while nothing matches there is no change to inform about
//! and nothing is sent.

use crate::compiled::CompiledCsrSubscription;
use derive_more::IsVariant;

/// How a Context Source Registration Subscription is notified.
#[derive(Clone, Copy, Debug, Eq, IsVariant, PartialEq)]
pub enum CsourceNotificationCadence {
    /// `timeInterval` is set: a notification is due on creation and on every
    /// elapsed interval, whether or not any registration matches.
    Periodic,
    /// `timeInterval` is absent: a notification reports a change to a matching
    /// registration, so it is due only while at least one matches.
    ChangeDriven,
}

impl CsourceNotificationCadence {
    /// The cadence `subscription` was created with.
    #[must_use]
    pub const fn of(subscription: &CompiledCsrSubscription) -> CsourceNotificationCadence {
        if subscription.time_interval_secs.is_some() {
            CsourceNotificationCadence::Periodic
        } else {
            CsourceNotificationCadence::ChangeDriven
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{compiled::CompiledCsrSubscription, csource_notification_cadence::CsourceNotificationCadence};
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use url::Url;

    fn subscription(time_interval_secs: Option<f64>) -> CompiledCsrSubscription {
        CompiledCsrSubscription {
            id: "urn:ngsi-ld:Subscription:cadence".parse().expect("valid URI"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: Vec::new(),
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: None,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs,
            jsonld_context: None,
            expires_at: None,
            compiled_csf: None,
        }
    }

    /// Clause 5.11.2.4: a `timeInterval` subscription is notified on schedule,
    /// "independent of any changes to the set of Context Source registrations".
    #[test]
    fn a_time_interval_subscription_is_periodic() {
        assert_eq!(CsourceNotificationCadence::of(&subscription(Some(10.0))), CsourceNotificationCadence::Periodic);
    }

    /// Clause 5.11.2.4: without a `timeInterval` the notification informs about
    /// a change of a matching registration.
    #[test]
    fn a_subscription_without_a_time_interval_is_change_driven() {
        assert_eq!(CsourceNotificationCadence::of(&subscription(None)), CsourceNotificationCadence::ChangeDriven);
    }
}
