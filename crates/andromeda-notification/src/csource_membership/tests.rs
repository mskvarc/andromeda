use crate::{
    csource_membership::{CsourceMembership, PlannedNotification},
    csource_transition::RegistrationChange,
    error::NotificationError,
    notified_registration_store::{NotifiedRegistrationStore, Release},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration, subscription::SubscriptionStatus};
use andromeda_subscription::{change::UpsertKind, compiled::CompiledCsrSubscription};
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use url::Url;

const VEHICLE: &str = "https://example.com/Vehicle";
const BUILDING: &str = "https://example.com/Building";
const SUBSCRIPTION: &str = "urn:ngsi-ld:Subscription:csource";
const REGISTRATION: &str = "urn:ngsi-ld:ContextSourceRegistration:1";
const OTHER_REGISTRATION: &str = "urn:ngsi-ld:ContextSourceRegistration:2";

/// The notified pairs one tenant database holds, shared by every planner built
/// over it as brokers share the database.
#[derive(Default)]
struct InMemoryLedger {
    /// `(subscription, registration)` pairs.
    pairs: Mutex<BTreeSet<(String, String)>>,
}

impl InMemoryLedger {
    fn pairs(&self) -> BTreeSet<(String, String)> {
        self.pairs.lock().expect("ledger lock").clone()
    }
}

impl NotifiedRegistrationStore for InMemoryLedger {
    fn claim<'a>(&'a self, subscription: &'a Uri, registrations: &'a [Uri]) -> BoxFuture<'a, Result<Vec<Uri>, NotificationError>> {
        Box::pin(async move {
            let mut pairs = self.pairs.lock().expect("ledger lock");
            Ok(registrations
                .iter()
                .filter(|registration| pairs.insert((subscription.to_string(), registration.to_string())))
                .cloned()
                .collect())
        })
    }

    fn release<'a>(&'a self, subscription: &'a Uri, registration: &'a Uri) -> BoxFuture<'a, Result<Release, NotificationError>> {
        Box::pin(async move {
            let removed = self
                .pairs
                .lock()
                .expect("ledger lock")
                .remove(&(subscription.to_string(), registration.to_string()));
            Ok(if removed { Release::Released } else { Release::Absent })
        })
    }

    fn notified_subscriptions<'a>(&'a self, registration: &'a Uri) -> BoxFuture<'a, Result<Vec<Uri>, NotificationError>> {
        Box::pin(async move {
            Ok(self
                .pairs
                .lock()
                .expect("ledger lock")
                .iter()
                .filter(|(_, notified)| notified == registration.as_str())
                .map(|(subscription, _)| uri(subscription))
                .collect())
        })
    }

    fn registrations_notified_to<'a>(&'a self, subscription: &'a Uri) -> BoxFuture<'a, Result<Vec<Uri>, NotificationError>> {
        Box::pin(async move {
            Ok(self
                .pairs
                .lock()
                .expect("ledger lock")
                .iter()
                .filter(|(owner, _)| owner == subscription.as_str())
                .map(|(_, registration)| uri(registration))
                .collect())
        })
    }
}

fn uri(raw: &str) -> Uri {
    raw.parse().expect("valid URI")
}

fn subscription_with(status: SubscriptionStatus, time_interval_secs: Option<f64>) -> Arc<CompiledCsrSubscription> {
    Arc::new(CompiledCsrSubscription {
        id: uri(SUBSCRIPTION),
        status,
        endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
        endpoint_accept: None,
        endpoint_receiver_info: None,
        endpoint_notifier_info: None,
        endpoint_cooldown: None,
        endpoint_timeout: None,
        selectors: vec![serde_json::from_value(json!({"type": VEHICLE})).expect("selector parses")],
        matching_attrs: Vec::new(),
        temporal_q: None,
        geo_q: None,
        scope_q: None,
        throttling_secs: None,
        time_interval_secs,
        jsonld_context: None,
        expires_at: None,
        compiled_csf: None,
    })
}

fn subscription() -> Arc<CompiledCsrSubscription> {
    subscription_with(SubscriptionStatus::Active, None)
}

fn registration(id: &str, raw_type: &str) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": raw_type}]}],
        "endpoint": "https://source.example.com/",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-02T00:00:00Z"
    }))
    .expect("registration parses")
}

/// One broker planning against `ledger`.
fn broker(ledger: &Arc<InMemoryLedger>) -> CsourceMembership {
    CsourceMembership::new(Arc::clone(ledger) as Arc<dyn NotifiedRegistrationStore>)
}

/// The reason and the carried registration's type of every planned
/// notification, in order.
fn summary(planned: &[PlannedNotification<'_>]) -> Vec<(CsrTriggerReason, String)> {
    planned
        .iter()
        .map(|notification| {
            let carried = serde_json::to_value(notification.registration).expect("registration serialises");
            (
                notification.reason,
                carried["information"][0]["entities"][0]["type"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn ids(registrations: &[&ContextSourceRegistration]) -> Vec<String> {
    registrations.iter().map(|registration| registration.id.to_string()).collect()
}

fn pair(registration: &str) -> (String, String) {
    (SUBSCRIPTION.to_owned(), registration.to_owned())
}

/// Clause 5.11.7: a registration committed just after its subscription, and
/// read by that subscription's initial notification, is notified once -- its
/// creation does not report it a second time.
#[tokio::test]
async fn a_registration_carried_by_the_initial_notification_is_not_reported_again_by_its_creation() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);
    let matching = [registration(REGISTRATION, VEHICLE)];

    let initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &matching)
        .await
        .expect("recorded")
        .carried;
    let on_creation = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(REGISTRATION),
            None,
            Some(&created),
        )
        .await;

    assert_eq!(ids(&initial), vec![REGISTRATION.to_owned()]);
    assert_eq!(summary(&on_creation), Vec::new());
}

/// A creation evaluated before the subscription's initial notification is
/// reported by the creation, and the initial notification leaves it out.
#[tokio::test]
async fn a_creation_evaluated_before_the_initial_notification_is_reported_once_by_the_creation() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);
    let matching = [registration(REGISTRATION, VEHICLE)];

    let on_creation = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(REGISTRATION),
            None,
            Some(&created),
        )
        .await;
    let initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &matching)
        .await
        .expect("recorded")
        .carried;

    assert_eq!(summary(&on_creation), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
    assert_eq!(ids(&initial), Vec::<String>::new());
}

/// Two brokers sharing the tenant database -- one sending a subscription's
/// initial notification, the other evaluating a registration change -- report
/// the registration once between them.
#[tokio::test]
async fn brokers_sharing_the_tenant_database_report_a_registration_once_between_them() {
    let ledger = Arc::new(InMemoryLedger::default());
    let initial_broker = broker(&ledger);
    let change_broker = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);
    let matching = [registration(REGISTRATION, VEHICLE)];

    let on_creation = change_broker
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(REGISTRATION),
            None,
            Some(&created),
        )
        .await;
    let initial = initial_broker
        .plan_upsert(&subscription, UpsertKind::Created, &matching)
        .await
        .expect("recorded");
    let current = registration(REGISTRATION, VEHICLE);
    let on_update = initial_broker
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Updated,
            &uri(REGISTRATION),
            Some(&created),
            Some(&current),
        )
        .await;

    assert_eq!(
        summary(&on_creation).len() + initial.carried.len(),
        1,
        "one notification reports the registration"
    );
    assert_eq!(summary(&on_update), vec![(CsrTriggerReason::Updated, VEHICLE.to_owned())]);
}

/// A subscription created on another broker is notified of a registration
/// change this broker evaluates: the record lives in the tenant database, not
/// in the broker that created the subscription.
#[tokio::test]
async fn a_subscription_created_on_another_broker_is_notified_of_a_change_evaluated_here() {
    let ledger = Arc::new(InMemoryLedger::default());
    let creating_broker = broker(&ledger);
    let other_broker = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);
    let current = registration(REGISTRATION, BUILDING);

    let _initial = creating_broker.plan_upsert(&subscription, UpsertKind::Created, &[]).await.expect("recorded");
    let on_creation = other_broker
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(REGISTRATION),
            None,
            Some(&created),
        )
        .await;
    let on_update = other_broker
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Updated,
            &uri(REGISTRATION),
            Some(&created),
            Some(&current),
        )
        .await;

    assert_eq!(summary(&on_creation), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
    assert_eq!(summary(&on_update), vec![(CsrTriggerReason::NoLongerMatching, VEHICLE.to_owned())]);
}

/// Clause 5.11.2.4: a matching registration created after the initial
/// notification is reported as `newlyMatching`.
#[tokio::test]
async fn a_registration_created_after_the_initial_notification_is_newly_matching() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);

    let initial = membership.plan_upsert(&subscription, UpsertKind::Created, &[]).await.expect("recorded").carried;
    let on_creation = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(REGISTRATION),
            None,
            Some(&created),
        )
        .await;

    assert_eq!(ids(&initial), Vec::<String>::new());
    assert_eq!(summary(&on_creation), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
}

/// Clause 5.3.3: a reported registration updated out of matching is
/// `noLongerMatching` and carries the state the subscriber was told matched;
/// a further update of it is silent.
#[tokio::test]
async fn a_reported_registration_updated_out_of_matching_is_no_longer_matching_once() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let matching = [registration(REGISTRATION, VEHICLE)];
    let before = registration(REGISTRATION, VEHICLE);
    let after = registration(REGISTRATION, BUILDING);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &matching).await.expect("recorded");
    let on_update = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&before), Some(&after))
        .await;
    let on_second_update = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&after), Some(&after))
        .await;

    assert_eq!(summary(&on_update), vec![(CsrTriggerReason::NoLongerMatching, VEHICLE.to_owned())]);
    assert_eq!(summary(&on_second_update), Vec::new());
    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// Clause 5.3.3: a registration updated into matching is `newlyMatching`, and
/// a reported one that still matches after an update is `updated`.
#[tokio::test]
async fn an_update_into_matching_is_newly_matching_and_a_further_matching_update_is_updated() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let building = registration(REGISTRATION, BUILDING);
    let vehicle = registration(REGISTRATION, VEHICLE);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &[]).await.expect("recorded");
    let into_matching = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&building), Some(&vehicle))
        .await;
    let still_matching = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&vehicle), Some(&vehicle))
        .await;

    assert_eq!(summary(&into_matching), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
    assert_eq!(summary(&still_matching), vec![(CsrTriggerReason::Updated, VEHICLE.to_owned())]);
}

/// Clause 5.11.7: deleting a reported registration is `noLongerMatching`
/// carrying the deleted state; deleting one never reported is silent.
#[tokio::test]
async fn deleting_a_reported_registration_is_no_longer_matching_and_an_unreported_one_is_silent() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let matching = [registration(REGISTRATION, VEHICLE)];
    let deleted = registration(REGISTRATION, VEHICLE);
    let never_reported = registration(OTHER_REGISTRATION, BUILDING);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &matching).await.expect("recorded");
    let on_delete = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Deleted, &uri(REGISTRATION), Some(&deleted), None)
        .await;
    let on_other_delete = membership
        .plan_registration_change(
            &subscriptions,
            RegistrationChange::Deleted,
            &uri(OTHER_REGISTRATION),
            Some(&never_reported),
            None,
        )
        .await;

    assert_eq!(summary(&on_delete), vec![(CsrTriggerReason::NoLongerMatching, VEHICLE.to_owned())]);
    assert_eq!(summary(&on_other_delete), Vec::new());
}

/// A reported registration deleted without a readable snapshot is withdrawn
/// but carries nothing, since Table 5.2.9-1 forbids an empty `information`.
#[tokio::test]
async fn a_deletion_without_a_snapshot_withdraws_the_registration_without_a_notification() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let matching = [registration(REGISTRATION, VEHICLE)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &matching).await.expect("recorded");
    let on_delete = membership
        .plan_registration_change(&[Arc::clone(&subscription)], RegistrationChange::Deleted, &uri(REGISTRATION), None, None)
        .await;

    assert_eq!(summary(&on_delete), Vec::new());
    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// A reported registration whose creation event finds it no longer matching
/// is left to the update that changed it, which carries the state the
/// subscriber was told; the creation carries nothing matching.
#[tokio::test]
async fn a_creation_found_no_longer_matching_is_left_to_the_update_that_changed_it() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let as_created = registration(REGISTRATION, VEHICLE);
    let current = registration(REGISTRATION, BUILDING);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &[registration(REGISTRATION, VEHICLE)])
        .await
        .expect("recorded");
    let on_creation = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Created, &uri(REGISTRATION), None, Some(&current))
        .await;
    let on_update = membership
        .plan_registration_change(
            &subscriptions,
            RegistrationChange::Updated,
            &uri(REGISTRATION),
            Some(&as_created),
            Some(&current),
        )
        .await;

    assert_eq!(summary(&on_creation), Vec::new());
    assert_eq!(summary(&on_update), vec![(CsrTriggerReason::NoLongerMatching, VEHICLE.to_owned())]);
}

/// Clause 5.11.3.4: an updated subscription is sent all current matches;
/// a record outside them that the updated subscription no longer selects is
/// withdrawn, so its later deletion is not reported.
#[tokio::test]
async fn a_subscription_update_resends_all_current_matches_and_withdraws_what_it_no_longer_selects() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let both = [registration(REGISTRATION, VEHICLE), registration(OTHER_REGISTRATION, VEHICLE)];
    let one = [registration(REGISTRATION, VEHICLE)];
    let dropped = registration(OTHER_REGISTRATION, BUILDING);

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &both).await.expect("recorded");
    let on_update = membership.plan_upsert(&subscription, UpsertKind::Updated, &one).await.expect("recorded");
    let beyond = membership.recorded_beyond(&subscription.id, &one).await.expect("read");
    let withdrawn = membership
        .withdraw_unselected(&subscription, vec![(uri(OTHER_REGISTRATION), Some(registration(OTHER_REGISTRATION, BUILDING)))])
        .await;
    let on_delete = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Deleted,
            &uri(OTHER_REGISTRATION),
            Some(&dropped),
            None,
        )
        .await;

    assert_eq!(ids(&on_update.carried), vec![REGISTRATION.to_owned()]);
    assert_eq!(beyond, vec![uri(OTHER_REGISTRATION)]);
    assert_eq!(withdrawn, vec![uri(OTHER_REGISTRATION)]);
    assert_eq!(summary(&on_delete), Vec::new());
    assert_eq!(ledger.pairs(), BTreeSet::from([pair(REGISTRATION)]));
}

/// A registration reported after an update read its match set, and still
/// selected, stays recorded: its next update is `updated`, not a second
/// `newlyMatching` (clause 5.3.3).
#[tokio::test]
async fn a_registration_reported_after_the_update_read_its_matches_stays_recorded() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let read_before = [registration(REGISTRATION, VEHICLE)];
    let reported_later = registration(OTHER_REGISTRATION, VEHICLE);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &read_before)
        .await
        .expect("recorded");
    let on_creation = membership
        .plan_registration_change(
            &subscriptions,
            RegistrationChange::Created,
            &uri(OTHER_REGISTRATION),
            None,
            Some(&reported_later),
        )
        .await;
    let _on_update = membership
        .plan_upsert(&subscription, UpsertKind::Updated, &read_before)
        .await
        .expect("recorded");
    let withdrawn = membership
        .withdraw_unselected(&subscription, vec![(uri(OTHER_REGISTRATION), Some(registration(OTHER_REGISTRATION, VEHICLE)))])
        .await;
    let on_later_update = membership
        .plan_registration_change(
            &subscriptions,
            RegistrationChange::Updated,
            &uri(OTHER_REGISTRATION),
            Some(&reported_later),
            Some(&reported_later),
        )
        .await;

    assert_eq!(summary(&on_creation), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
    assert_eq!(withdrawn, Vec::<Uri>::new());
    assert_eq!(summary(&on_later_update), vec![(CsrTriggerReason::Updated, VEHICLE.to_owned())]);
}

/// An update notification that was not sent withdraws what it newly recorded
/// and restores what it withdrew, leaving the record as the subscriber last
/// saw it.
#[tokio::test]
async fn an_unsent_update_notification_leaves_the_record_as_the_subscriber_last_saw_it() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let before = [registration(REGISTRATION, VEHICLE)];
    let after = [registration(OTHER_REGISTRATION, VEHICLE)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &before).await.expect("recorded");
    let on_update = membership.plan_upsert(&subscription, UpsertKind::Updated, &after).await.expect("recorded");
    let withdrawn = membership
        .withdraw_unselected(&subscription, vec![(uri(REGISTRATION), Some(registration(REGISTRATION, BUILDING)))])
        .await;
    membership.withdraw_unsent(&subscription.id, &on_update.recorded).await;
    membership.restore(&subscription.id, &withdrawn).await;

    assert_eq!(ledger.pairs(), BTreeSet::from([pair(REGISTRATION)]));
}

/// Clause 5.11.2.4: a `timeInterval` subscription gets every current match on
/// upsert, records nothing, and is not notified by registration changes.
#[tokio::test]
async fn a_time_interval_subscription_is_sent_every_match_and_ignores_registration_changes() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription_with(SubscriptionStatus::Active, Some(10.0));
    let matching = [registration(REGISTRATION, VEHICLE)];
    let created = registration(OTHER_REGISTRATION, VEHICLE);

    let first = membership
        .plan_upsert(&subscription, UpsertKind::Created, &matching)
        .await
        .expect("recorded")
        .carried;
    let second = membership
        .plan_upsert(&subscription, UpsertKind::Created, &matching)
        .await
        .expect("recorded")
        .carried;
    let on_creation = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Created,
            &uri(OTHER_REGISTRATION),
            None,
            Some(&created),
        )
        .await;

    assert_eq!(ids(&first), vec![REGISTRATION.to_owned()]);
    assert_eq!(ids(&second), vec![REGISTRATION.to_owned()]);
    assert_eq!(summary(&on_creation), Vec::new());
    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// Clause 5.8.6: a paused subscription is not notified of a registration
/// change, and its record is left as it was.
#[tokio::test]
async fn a_paused_subscription_is_not_notified_of_a_registration_change() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let paused = subscription_with(SubscriptionStatus::Paused, None);
    let created = registration(REGISTRATION, VEHICLE);

    let on_creation = membership
        .plan_registration_change(&[paused], RegistrationChange::Created, &uri(REGISTRATION), None, Some(&created))
        .await;

    assert_eq!(summary(&on_creation), Vec::new());
    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// A `newlyMatching` notification held back by throttling or cooldown is
/// withdrawn again, so the next change reports the registration as
/// `newlyMatching` rather than `updated`.
#[tokio::test]
async fn an_unsent_newly_matching_notification_is_withdrawn_from_the_record() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let created = registration(REGISTRATION, VEHICLE);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership.plan_upsert(&subscription, UpsertKind::Created, &[]).await.expect("recorded");
    let on_creation = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Created, &uri(REGISTRATION), None, Some(&created))
        .await;
    for planned in &on_creation {
        membership.undo(planned).await;
    }
    let on_update = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&created), Some(&created))
        .await;

    assert_eq!(summary(&on_update), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
}

/// A `noLongerMatching` notification held back is restored to the record, so
/// the subscriber, still believing the registration matches, is told when it
/// next changes.
#[tokio::test]
async fn an_unsent_no_longer_matching_notification_is_restored_to_the_record() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let vehicle = registration(REGISTRATION, VEHICLE);
    let building = registration(REGISTRATION, BUILDING);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &[registration(REGISTRATION, VEHICLE)])
        .await
        .expect("recorded");
    let on_update = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Updated, &uri(REGISTRATION), Some(&vehicle), Some(&building))
        .await;
    for planned in &on_update {
        membership.undo(planned).await;
    }

    assert_eq!(ledger.pairs(), BTreeSet::from([pair(REGISTRATION)]));
}

/// An initial notification that was never sent withdraws what it recorded,
/// so the registrations are reported by their next change.
#[tokio::test]
async fn an_unsent_initial_notification_withdraws_what_it_recorded() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let matching = [registration(REGISTRATION, VEHICLE)];

    let initial = membership.plan_upsert(&subscription, UpsertKind::Created, &matching).await.expect("recorded");
    membership.withdraw_unsent(&subscription.id, &initial.recorded).await;

    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// A registration updated out of matching between the initial notification's
/// read of the match set and its recording -- its update evaluated first, with
/// nothing recorded to withdraw -- is withdrawn by the re-read and left out of
/// the notification, so the subscriber is never told it matches.
#[tokio::test]
async fn a_registration_changed_before_the_initial_recording_is_withdrawn_and_left_out() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let read_with_the_match_set = [registration(REGISTRATION, VEHICLE)];
    let as_updated = registration(REGISTRATION, BUILDING);

    let on_update = membership
        .plan_registration_change(
            &[Arc::clone(&subscription)],
            RegistrationChange::Updated,
            &uri(REGISTRATION),
            Some(&read_with_the_match_set[0]),
            Some(&as_updated),
        )
        .await;
    let _recorded = membership
        .plan_upsert(&subscription, UpsertKind::Created, &read_with_the_match_set)
        .await
        .expect("recorded");
    let confirmed = membership
        .confirm(&subscription, vec![(uri(REGISTRATION), Some(registration(REGISTRATION, BUILDING)))])
        .await;

    assert_eq!(summary(&on_update), Vec::new());
    assert!(confirmed.is_empty(), "the stale match is not notified");
    assert_eq!(ledger.pairs(), BTreeSet::new());
}

/// A recorded registration deleted before the re-read is withdrawn; one that
/// still matches is confirmed with the state the store holds now.
#[tokio::test]
async fn the_re_read_withdraws_a_deleted_registration_and_confirms_a_matching_one_as_it_stands() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let matching = [registration(REGISTRATION, VEHICLE), registration(OTHER_REGISTRATION, VEHICLE)];

    let _recorded = membership.plan_upsert(&subscription, UpsertKind::Created, &matching).await.expect("recorded");
    let confirmed = membership
        .confirm(
            &subscription,
            vec![
                (uri(REGISTRATION), None),
                (uri(OTHER_REGISTRATION), Some(registration(OTHER_REGISTRATION, VEHICLE))),
            ],
        )
        .await;

    assert_eq!(
        confirmed.iter().map(|registration| registration.id.to_string()).collect::<Vec<_>>(),
        vec![OTHER_REGISTRATION.to_owned()]
    );
    assert_eq!(ledger.pairs(), BTreeSet::from([pair(OTHER_REGISTRATION)]));
}

/// A held-back `noLongerMatching` for a deleted registration is not restored:
/// a registration later created under the same identifier is reported as
/// `newlyMatching`, not taken for one already notified.
#[tokio::test]
async fn an_unsent_withdrawal_of_a_deleted_registration_is_not_restored() {
    let ledger = Arc::new(InMemoryLedger::default());
    let membership = broker(&ledger);
    let subscription = subscription();
    let deleted = registration(REGISTRATION, VEHICLE);
    let recreated = registration(REGISTRATION, VEHICLE);
    let subscriptions = [Arc::clone(&subscription)];

    let _initial = membership
        .plan_upsert(&subscription, UpsertKind::Created, &[registration(REGISTRATION, VEHICLE)])
        .await
        .expect("recorded");
    let on_delete = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Deleted, &uri(REGISTRATION), Some(&deleted), None)
        .await;
    for planned in &on_delete {
        membership.undo(planned).await;
    }
    let on_recreation = membership
        .plan_registration_change(&subscriptions, RegistrationChange::Created, &uri(REGISTRATION), None, Some(&recreated))
        .await;

    assert_eq!(summary(&on_recreation), vec![(CsrTriggerReason::NewlyMatching, VEHICLE.to_owned())]);
}
