//! Since when a Subscription may be notified of a change, and which committed
//! changes that leaves owed to it.
//!
//! Clause 5.8.6 sends a notification "if and only if" the Subscription is
//! active, and Table 5.2.12-1 delivers none while `isActive` is false. A
//! change committed while the Subscription was paused is therefore never owed
//! to it, however late the change is evaluated. The selection members decide
//! the trigger at the change itself: `watchedAttributes` are those that
//! "trigger a new notification when they are changed", `notificationTrigger`
//! lists "what kind of changes shall trigger", and `q` and `geoQ` "shall be met
//! ... in order to trigger the notification". A change is therefore judged by
//! the selection as it stood when it was committed, and one judged later than
//! its commit cannot be judged by a selection written after it.
//!
//! Both instants are the store's transaction timestamps, as is the commit
//! instant of every change they are compared with, so brokers sharing the
//! tenant database order them alike.

use andromeda_context_commit::store_instant::StoreInstant;
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    subscription::{Subscription, SubscriptionStatus},
    subscription_request::SubscriptionRequest,
};

/// The instants a Subscription last became able to be notified at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotifyingSince {
    /// When the Subscription was created active or last set active from
    /// paused or expired.
    pub active: StoreInstant,
    /// When the Subscription was created or the value of what it selects last
    /// changed.
    pub selected: StoreInstant,
}

/// Whether a change was matched against the Subscription's selection when it
/// was committed, or is matched against it only when it is evaluated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionJudged {
    /// The commit matched the change against the selection as it stood then.
    AtCommit,
    /// The change is matched against the selection as it stands when it is
    /// evaluated, which must not be newer than the change.
    AtEvaluation,
}

impl NotifyingSince {
    /// Whether a change committed at `committed_at` is owed to the
    /// Subscription: it was active when the change committed, and, unless the
    /// commit itself matched the change, its selection is the one that stood
    /// then.
    #[must_use]
    pub fn owes(self, committed_at: StoreInstant, judged: SelectionJudged) -> bool {
        let active = committed_at >= self.active;
        match judged {
            SelectionJudged::AtCommit => active,
            SelectionJudged::AtEvaluation => active && committed_at >= self.selected,
        }
    }
}

/// Whether one update restarts an instant of [`NotifyingSince`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinceRestart {
    /// The instant stays as stored.
    Kept,
    /// The instant becomes the update's transaction timestamp.
    Restarted,
}

impl SinceRestart {
    /// Whether the update fragment `request` changes the value of what the
    /// `existing` Subscription selects: `entities`, `watchedAttributes`,
    /// `notificationTrigger`, `q`, `geoQ`, `scopeQ`, `csf` or `localOnly`
    /// (Table 5.2.12-1), each compared after its terms were expanded.
    #[must_use]
    pub fn of_selection(request: &SubscriptionRequest, existing: &Subscription) -> SinceRestart {
        let selection = &existing.selection;
        let filters = &existing.filters;
        let changed = request.entities.changes(selection.entities.as_ref())
            || request.watched_attributes.changes(selection.watched_attributes.as_ref())
            || request.notification_trigger.changes(selection.notification_trigger.as_ref())
            || request.q.changes(filters.q.as_ref())
            || request.geo_q.changes(filters.geo_q.as_ref())
            || request.scope_q.changes(filters.scope_q.as_ref())
            || request.csf.changes(filters.csf.as_ref())
            || local_only_changes(&request.local_only, existing.delivery.local_only);
        if changed { SinceRestart::Restarted } else { SinceRestart::Kept }
    }
}

/// Whether `requested` changes the effective `localOnly`, which an absent or
/// deleted member leaves at its Table 5.2.12-1 default, `false`.
fn local_only_changes(requested: &Clearable<bool>, stored: Option<bool>) -> bool {
    let before = stored.unwrap_or(false);
    let after = match requested {
        Clearable::Absent => before,
        Clearable::NgsiLdNull => false,
        Clearable::Set(local_only) => *local_only,
    };
    before != after
}

/// What one update restarts of a Subscription's [`NotifyingSince`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotifyingRestart {
    /// Whether the update made the Subscription active again.
    pub active: SinceRestart,
    /// Whether the update changed the value of what the Subscription selects.
    pub selected: SinceRestart,
}

impl NotifyingRestart {
    /// What an update moving the Subscription from `previous` to `next`, its
    /// selection changing as `selection` says, restarts: the active instant
    /// only when it leaves paused or expired for active (clause 5.8.2.4).
    #[must_use]
    pub const fn of_update(previous: SubscriptionStatus, next: SubscriptionStatus, selection: SinceRestart) -> NotifyingRestart {
        let active = match (previous, next) {
            (SubscriptionStatus::Paused | SubscriptionStatus::Expired, SubscriptionStatus::Active) => SinceRestart::Restarted,
            (SubscriptionStatus::Active, SubscriptionStatus::Active | SubscriptionStatus::Paused | SubscriptionStatus::Expired)
            | (SubscriptionStatus::Paused | SubscriptionStatus::Expired, SubscriptionStatus::Paused | SubscriptionStatus::Expired) => SinceRestart::Kept,
        };
        NotifyingRestart { active, selected: selection }
    }
}

#[cfg(test)]
mod tests {
    use crate::notifying_since::{NotifyingRestart, NotifyingSince, SelectionJudged, SinceRestart};
    use andromeda_context_commit::store_instant::StoreInstant;
    use andromeda_ngsi_ld_model::{
        request_body::RequestBody,
        subscription::{Subscription, SubscriptionStatus},
        subscription_request::SubscriptionRequest,
    };
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use chrono::DateTime;
    use serde_json::{Value, json};

    fn at(unix_seconds: i64) -> StoreInstant {
        StoreInstant::new(DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range"))
    }

    fn since(active: i64, selected: i64) -> NotifyingSince {
        NotifyingSince {
            active: at(active),
            selected: at(selected),
        }
    }

    /// Table 5.2.12-1 `isActive`: a change committed while the Subscription
    /// was paused, here before it was set active again at 2 000, is owed
    /// nothing however it was matched.
    #[test]
    fn a_change_committed_before_the_subscription_became_active_is_not_owed() {
        for judged in [SelectionJudged::AtCommit, SelectionJudged::AtEvaluation] {
            assert!(!since(2_000, 1_000).owes(at(1_500), judged));
        }
    }

    #[test]
    fn a_change_committed_once_the_subscription_is_active_is_owed() {
        for judged in [SelectionJudged::AtCommit, SelectionJudged::AtEvaluation] {
            assert!(since(2_000, 1_000).owes(at(2_500), judged));
            assert!(since(2_000, 1_000).owes(at(2_000), judged), "the activation instant itself is owed");
        }
    }

    /// Clause 5.2.12 has no initial notification: a Subscription created at
    /// 2 000 is both active and selecting from then on.
    #[test]
    fn a_change_committed_before_the_subscription_was_created_is_not_owed() {
        assert!(!since(2_000, 2_000).owes(at(1_999), SelectionJudged::AtEvaluation));
        assert!(!since(2_000, 2_000).owes(at(1_999), SelectionJudged::AtCommit));
    }

    /// A change committed before the selection last changed is judged by the
    /// selection of its commit only when the commit matched it; evaluated
    /// later, the selection now standing did not exist when it happened.
    #[test]
    fn a_selection_changed_after_the_commit_drops_only_a_change_evaluated_later() {
        let reselected = since(1_000, 2_000);
        assert!(!reselected.owes(at(1_500), SelectionJudged::AtEvaluation));
        assert!(reselected.owes(at(1_500), SelectionJudged::AtCommit));
        assert!(reselected.owes(at(2_500), SelectionJudged::AtEvaluation));
    }

    /// Clause 5.8.2.4: only an update that leaves paused or expired for active
    /// restarts the active instant.
    #[test]
    fn only_a_return_to_active_restarts_the_active_instant() {
        use SubscriptionStatus::{Active, Expired, Paused};
        let restarted = |previous, next| NotifyingRestart::of_update(previous, next, SinceRestart::Kept).active;
        pretty_assertions::assert_eq!(restarted(Paused, Active), SinceRestart::Restarted);
        pretty_assertions::assert_eq!(restarted(Expired, Active), SinceRestart::Restarted);
        for (previous, next) in [(Active, Active), (Active, Paused), (Paused, Paused), (Expired, Expired), (Paused, Expired)] {
            pretty_assertions::assert_eq!(restarted(previous, next), SinceRestart::Kept, "{previous:?} to {next:?}");
        }
    }

    #[test]
    fn the_selection_restart_is_the_one_the_update_reports() {
        let restart = NotifyingRestart::of_update(SubscriptionStatus::Active, SubscriptionStatus::Active, SinceRestart::Restarted);
        pretty_assertions::assert_eq!(restart.selected, SinceRestart::Restarted);
        pretty_assertions::assert_eq!(restart.active, SinceRestart::Kept);
    }

    /// An entity subscription as it stands in the store.
    fn stored() -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:stored",
            "type": "Subscription",
            "entities": [{"type": "https://example.com/Vehicle"}],
            "watchedAttributes": ["https://example.com/speed"],
            "q": "speed>1",
            "notification": {"endpoint": {"uri": "http://receiver.example.com/notify"}},
            "status": "active",
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-01T00:00:00Z"
        }))
        .expect("the fixture is a valid subscription")
    }

    fn restart_of(fragment: &Value) -> SinceRestart {
        let request = SubscriptionRequest::from_request_text(fragment.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable request");
        SinceRestart::of_selection(&request, &stored())
    }

    /// A member left out, or written with the value it holds, leaves the
    /// selection as it was.
    #[test]
    fn an_update_that_leaves_the_selection_as_it_was_keeps_the_selected_instant() {
        for fragment in [
            json!({}),
            json!({"isActive": false, "throttling": 5}),
            json!({"q": "speed>1", "watchedAttributes": ["https://example.com/speed"]}),
            json!({"entities": [{"type": "https://example.com/Vehicle"}]}),
            json!({"localOnly": false}),
            json!({"localOnly": "urn:ngsi-ld:null"}),
            json!({"geoQ": "urn:ngsi-ld:null"}),
        ] {
            pretty_assertions::assert_eq!(restart_of(&fragment), SinceRestart::Kept, "{fragment}");
        }
    }

    /// Table 5.2.12-1: each of these members decides which changes trigger a
    /// notification, so a new value restarts the selected instant.
    #[test]
    fn an_update_that_changes_what_the_subscription_selects_restarts_the_selected_instant() {
        for fragment in [
            json!({"q": "speed>2"}),
            json!({"q": "urn:ngsi-ld:null"}),
            json!({"entities": [{"type": "https://example.com/Truck"}]}),
            json!({"watchedAttributes": ["https://example.com/heading"]}),
            json!({"notificationTrigger": ["entityCreated"]}),
            json!({"geoQ": {"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": [1, 2]}}),
            json!({"scopeQ": "/Madrid"}),
            json!({"csf": "brand==\"ACME\""}),
            json!({"localOnly": true}),
        ] {
            pretty_assertions::assert_eq!(restart_of(&fragment), SinceRestart::Restarted, "{fragment}");
        }
    }
}
