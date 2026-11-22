//! Delivering a durably queued notification event through the processor.
//!
//! A queued delivery carries the event as it was persisted, so the change
//! event the processor evaluates is rebuilt from the stored row: the entity's
//! types come back from the state it carries and its system attributes from
//! the event, which recorded them as the change left the Entity.
//!
//! A `throttling` or `cooldown` window that withholds a queued notification
//! is judged by the change that raised it, not by how long it waited in the
//! queue:
//!
//! - Table 5.2.15-1 sends no notification for a request received while the
//!   cooldown runs, so a delivery whose change committed at or after the
//!   failure that opened the cooldown is withheld for good. A failure after
//!   the commit opened the window while the delivery waited, and the cooldown
//!   elapses "before attempting to make a subsequent notification", so the
//!   delivery waits it out.
//! - Clause 5.2.12 lets one notification through per `throttling` window. A
//!   delivery the window withholds is withheld for good when a newer delivery
//!   of its subscription is queued, which notifies a later change, and waits
//!   the window out when it is the newest. However long the queue backed up
//!   behind an endpoint that failed, the latest change goes out within one
//!   window and the ones before it are dropped, as live changes inside the
//!   window are.
//! - A retry was withheld by the window its own failed attempt opened, and
//!   the subscription's newer deliveries wait behind it, so it waits the
//!   window out.

use crate::{
    delivery_entry::{DeliveryEntry, NewerDelivery},
    delivery_transport::{DeliveryDisposition, NotificationDeliveryTransport},
    notification_event::NotificationEvent,
    processor::{
        NotificationEvaluationOutcome,
        NotificationProcessor,
        change_audience::{ChangeAudience, CommitFilters},
        delivery_failure::DeliveryFailure,
        notification_identity::NotificationIdentity,
    },
};
use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant};
use andromeda_delivery::failure_class::FailureClass;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::ChangeOrigin};
use andromeda_subscription::{error::SubscriptionError, notification_slot::Withholding};
use futures::future::BoxFuture;
use std::sync::Arc;

impl NotificationDeliveryTransport for NotificationProcessor {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            let event = queued_change_event(&entry.event);
            if let Err(error) = self.refresh_cached_subscription(&entry.target, &entry.subscription_id).await {
                return unreadable_subscription(&entry.subscription_id, error);
            }
            let audience = ChangeAudience::Queued {
                subscription: &entry.subscription_id,
                filters: commit_filters(&entry.event),
            };
            let identity = NotificationIdentity::Queued {
                write_id: entry.event.write_id,
                event_ordinal: entry.event.event_ordinal,
            };
            let outcome = self
                .apply_entity_change_for(entry.target.clone(), event, entry.event.committed_at, audience, identity)
                .await;
            disposition(outcome, entry.attempt_count, entry.event.committed_at, entry.newer)
        })
    }
}

/// How the queue settles a delivery that had failed `attempts_before`
/// attempts, whose change committed at `committed_at` and which had a
/// `newer` delivery queued or not, when its notification came to `outcome`.
///
/// A failure is settled under its class, an unavailable endpoint or store
/// first since it explains the attempt best, with the longest wait an
/// endpoint of that class asked for. A notification a window withheld
/// is settled by [`withheld`], and one owed nothing is obsolete.
fn disposition(outcome: NotificationEvaluationOutcome, attempts_before: i32, committed_at: StoreInstant, newer: NewerDelivery) -> DeliveryDisposition {
    match outcome {
        NotificationEvaluationOutcome { delivered: 1.., .. } => DeliveryDisposition::Delivered,
        NotificationEvaluationOutcome { unavailable: Some(failed), .. } => DeliveryDisposition::Failed {
            class: FailureClass::Unavailable,
            retry_after: failed.retry_after,
            detail: DeliveryFailure::TransientEndpoints(failed).to_string(),
        },
        NotificationEvaluationOutcome {
            server_error: Some(failed), ..
        } => DeliveryDisposition::Failed {
            class: FailureClass::ServerError,
            retry_after: failed.retry_after,
            detail: DeliveryFailure::ServerErrorEndpoints(failed).to_string(),
        },
        NotificationEvaluationOutcome { permanent: Some(failed), .. } => DeliveryDisposition::Failed {
            class: FailureClass::Permanent,
            retry_after: failed.retry_after,
            detail: DeliveryFailure::PermanentEndpoints(failed).to_string(),
        },
        NotificationEvaluationOutcome {
            withheld: Some(withholding), ..
        } => withheld(withholding, attempts_before, committed_at, newer),
        NotificationEvaluationOutcome { .. } => DeliveryDisposition::Obsolete,
    }
}

/// How the queue settles a delivery the windows of `withholding` withheld,
/// that had failed `attempts_before` attempts, whose change committed at
/// `committed_at` and which had a `newer` delivery queued or not.
///
/// It is withheld for good when its change committed inside the cooldown, or
/// when the throttling window withheld it and a newer delivery is queued;
/// otherwise it waits until the windows have passed. A retry always waits.
fn withheld(withholding: Withholding, attempts_before: i32, committed_at: StoreInstant, newer: NewerDelivery) -> DeliveryDisposition {
    let waits = DeliveryDisposition::Deferred {
        window: withholding.reopens_after,
    };
    if attempts_before > 0 {
        return waits;
    }
    let committed_inside_cooldown = withholding.cooled_since.is_some_and(|failed_at| failed_at <= committed_at.get());
    let superseded = withholding.throttled_since.is_some() && newer == NewerDelivery::Queued;
    if committed_inside_cooldown || superseded {
        DeliveryDisposition::Obsolete
    } else {
        waits
    }
}

/// The disposition of a delivery whose subscription could not be read back.
///
/// Without the subscription as it now stands the change cannot be matched
/// against it, and reading the failure as a subscription that is gone would
/// acknowledge the delivery as obsolete, losing a notification that clause 5.8.6
/// owes the subscriber. A store that could not answer may answer the next
/// attempt, so the delivery is retried. No notification was attempted, so no
/// subscription bookkeeping (clause 5.2.14.2 `timesSent`, `timesFailed`)
/// records it.
fn unreadable_subscription(subscription_id: &Identifier, error: SubscriptionError) -> DeliveryDisposition {
    DeliveryDisposition::Failed {
        class: FailureClass::Unavailable,
        detail: DeliveryFailure::SubscriptionUnreadable {
            subscription_id: subscription_id.clone(),
            source: error,
        }
        .to_string(),
        retry_after: None,
    }
}

/// Whether the commit decided the entity filters of the queued delivery.
///
/// The commit reads the entity filters on the states the change carries: a
/// deletion on the state before it or the state it removed, any other change on
/// the state it left and, when it removed instances, the state before it for a
/// subscription the removal is owed to. A
/// queued event carrying one of those states was filtered on it; one carrying
/// none was admitted without a state to read.
fn commit_filters(stored: &NotificationEvent) -> CommitFilters {
    let filtered = match stored.change.lifecycle {
        EntityLifecycle::Deleted => stored.entity_snapshot.is_some() || stored.previous_snapshot.is_some(),
        EntityLifecycle::Created | EntityLifecycle::Modified => {
            stored.entity_snapshot.is_some() || (stored.previous_snapshot.is_some() && stored.change.deletes_instances())
        }
    };
    if filtered { CommitFilters::Decided } else { CommitFilters::Undecided }
}

/// Rebuild the change a durably queued delivery records, as the processor
/// evaluates it.
///
/// The stored lifecycle says whether the Entity outlived the change, so the one
/// stored state is placed accordingly: an Entity the change deleted is carried
/// as what was removed (the shape clause 5.8.6 reduces to `deletedAt`), any
/// other Entity as the write left it. Either way the clause 4.8 system
/// attributes are the ones the event recorded, never the ones the row holds by
/// delivery time: a later write may have moved its `modifiedAt` on, and a
/// deleted row is gone.
fn queued_change_event(stored: &NotificationEvent) -> ChangeEvent {
    let snapshot = stored.entity_snapshot.as_ref().map(Arc::clone);
    // clone: the types are copied out before the snapshot moves into the event
    let entity_types = snapshot.as_ref().map_or_default(|state| state.types().as_slice().to_vec());
    let (removed, current) = match stored.change.lifecycle {
        EntityLifecycle::Deleted => (snapshot, None),
        EntityLifecycle::Created | EntityLifecycle::Modified => (None, snapshot),
    };
    ChangeEvent::builder()
        .entity_id(stored.entity_id.clone())
        .entity_types(entity_types)
        .change(stored.change.clone())
        .occurred_at(stored.created_at)
        .entity_snapshot(removed)
        .previous_attributes(stored.previous_snapshot.as_ref().map(Arc::clone))
        .sys_attrs_snapshot(stored.sys_attrs_snapshot.clone())
        .origin(ChangeOrigin::Local)
        .current_payload(current)
        .build()
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_entry::NewerDelivery,
        delivery_transport::DeliveryDisposition,
        notification_event::{NotificationEvent, NotificationEventKey},
        processor::{
            NotificationEvaluationOutcome,
            attempt_failure::{FailedAttempts, NotificationAttemptFailure},
            change_audience::CommitFilters,
            durable_delivery::{commit_filters, disposition, queued_change_event, unreadable_subscription},
        },
    };
    use andromeda_context_commit::{store_instant::StoreInstant, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_delivery::{endpoint_failure::EndpointFailure, failure_class::FailureClass, outcome::EndpointRejection};
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::SysAttrsSnapshot,
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription::{error::SubscriptionError, notification_slot::Withholding};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{sync::Arc, time::Duration};
    use uuid::Uuid;

    /// The attribute the deliveries below name.
    const NAME: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";

    /// Decode a stored document into a state.
    fn state(document: &Value) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes"))
    }

    /// An Entity carrying `name` and `airQualityLevel`.
    fn entity_with_name() -> Arc<EntityState> {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            NAME: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Tower"}]}],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }]))
    }

    /// The same Entity as a clause 5.6.5 Attribute deletion leaves it.
    fn entity_without_name() -> Arc<EntityState> {
        state(&json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
            "https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]
            }]
        }]))
    }

    /// The system attributes a change recorded.
    fn recorded_instants() -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant"),
            modified_at: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).single().expect("valid instant"),
            expires_at: None,
        }
    }

    /// One queued delivery of a change through `lifecycle` that deleted the
    /// `name` Attribute.
    fn queued(lifecycle: EntityLifecycle, snapshot: Option<Arc<EntityState>>, sys_attrs: Option<SysAttrsSnapshot>) -> NotificationEvent {
        let name = AttributeChange {
            key: AttributeInstanceKey {
                attr: NAME.parse().expect("valid attribute name"),
                dataset_id: None,
            },
            kind: AttributeChangeKind::Deleted,
        };
        NotificationEvent::builder()
            .key(NotificationEventKey::from(1_i64))
            .write_id(WriteId::from_uuid(Uuid::nil()))
            .event_ordinal(WriteOrdinal::at(0))
            .change(EntityChange::new(lifecycle, vec![name]))
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("valid entity URI"))
            .entity_snapshot(snapshot)
            .sys_attrs_snapshot(sys_attrs)
            .created_at(Utc::now())
            .committed_at(StoreInstant::new(Utc::now()))
            .build()
    }

    /// Clause 5.6.5 removes one Attribute and leaves the Entity: the queued
    /// change is a modification, rebuilt as the Entity the write left behind,
    /// so clause 5.8.6 notifies the deleted Attribute as `urn:ngsi-ld:null`
    /// rather than reducing the Entity to `deletedAt`. It carries the system
    /// attributes the write left the Entity with, not the ones its row holds
    /// by delivery time (clause 4.8 `modifiedAt` is that write's instant).
    #[test]
    fn a_queued_attribute_deletion_is_rebuilt_as_the_entity_it_left_behind() {
        let left_behind = entity_without_name();
        let event = queued_change_event(&queued(EntityLifecycle::Modified, Some(Arc::clone(&left_behind)), Some(recorded_instants())));
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "urn:ngsi-ld:Building:1");
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Modified);
        assert!(event.current_payload.as_ref().is_some_and(|current| Arc::ptr_eq(current, &left_behind)));
        assert!(event.entity_snapshot.is_none());
        pretty_assertions::assert_eq!(event.sys_attrs_snapshot, Some(recorded_instants()));
    }

    /// Clause 5.6.6 removes the Entity: the queued change is a deletion,
    /// rebuilt as what was removed with the system attributes the event
    /// recorded, whatever instances it names.
    #[test]
    fn a_queued_entity_deletion_is_rebuilt_as_what_was_removed() {
        let removed = entity_with_name();
        let event = queued_change_event(&queued(EntityLifecycle::Deleted, Some(Arc::clone(&removed)), Some(recorded_instants())));
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Deleted);
        assert!(event.current_payload.is_none());
        assert!(event.entity_snapshot.as_ref().is_some_and(|snapshot| Arc::ptr_eq(snapshot, &removed)));
        pretty_assertions::assert_eq!(event.sys_attrs_snapshot, Some(recorded_instants()));
        pretty_assertions::assert_eq!(
            event.entity_types.iter().map(EntityTypeName::as_str).collect::<Vec<_>>(),
            vec!["https://uri.etsi.org/ngsi-ld/default-context/Building"]
        );
    }

    /// A delivery with no snapshot leaves the Entity to be read from the
    /// store, and its types unknown.
    #[test]
    fn a_queued_modification_without_a_snapshot_leaves_the_entity_to_be_read() {
        let event = queued_change_event(&queued(EntityLifecycle::Modified, None, None));
        assert!(event.current_payload.is_none());
        assert!(event.entity_snapshot.is_none());
        assert_eq!(event.entity_types, Vec::<EntityTypeName>::new());
    }

    /// A delivery queued with the state the commit filtered on was decided
    /// at commit; one queued without any state was admitted undecided.
    #[test]
    fn a_queued_state_carries_the_commit_filter_decision() {
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Modified, Some(entity_without_name()), None)),
            CommitFilters::Decided
        ));
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Deleted, Some(entity_with_name()), None)),
            CommitFilters::Decided
        ));
        assert!(matches!(
            commit_filters(&queued(EntityLifecycle::Modified, None, None)),
            CommitFilters::Undecided
        ));
    }

    /// A subscription the store could not read back is not a subscription that
    /// is gone: the delivery is retried rather than acknowledged as obsolete,
    /// which would lose the notification it owes.
    #[test]
    fn a_subscription_the_store_cannot_read_back_retries_the_delivery() {
        let subscription_id = "urn:ngsi-ld:Subscription:1".parse().expect("valid subscription URI");
        let disposition = unreadable_subscription(&subscription_id, SubscriptionError::from(PersistenceFailure::Unavailable));
        match disposition {
            DeliveryDisposition::Failed {
                class: FailureClass::Unavailable,
                detail,
                retry_after: None,
            } => {
                assert!(detail.contains("urn:ngsi-ld:Subscription:1"), "{detail}");
                assert!(detail.contains("the datastore is temporarily unavailable"), "{detail}");
            }
            other @ (DeliveryDisposition::Delivered
            | DeliveryDisposition::Obsolete
            | DeliveryDisposition::Failed { .. }
            | DeliveryDisposition::Deferred { .. }) => {
                panic!("an unreadable subscription must be retried, got {other:?}")
            }
        }
    }

    /// The outcome of a change that delivered nothing and failed nothing.
    fn quiet() -> NotificationEvaluationOutcome {
        NotificationEvaluationOutcome::default()
    }

    #[test]
    fn a_failure_is_settled_under_its_class_an_unavailable_one_first() {
        let unavailable = || Some(FailedAttempts::new(NotificationAttemptFailure::ContextUnloadable));
        let server_error = || {
            Some(FailedAttempts::new(NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
                reason: EndpointRejection::HttpStatus {
                    status: 500,
                    retry_after: None,
                },
                class: FailureClass::ServerError,
            })))
        };
        let permanent = || Some(FailedAttempts::new(NotificationAttemptFailure::NoData));
        let class_of = |outcome| match disposition(outcome, 0, committed(), NewerDelivery::Absent) {
            DeliveryDisposition::Failed { class, .. } => Some(class),
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete | DeliveryDisposition::Deferred { .. } => None,
        };
        let all = NotificationEvaluationOutcome {
            unavailable: unavailable(),
            server_error: server_error(),
            permanent: permanent(),
            ..quiet()
        };
        assert_eq!(class_of(all), Some(FailureClass::Unavailable));
        let without_unavailable = NotificationEvaluationOutcome {
            server_error: server_error(),
            permanent: permanent(),
            ..quiet()
        };
        assert_eq!(class_of(without_unavailable), Some(FailureClass::ServerError));
        let permanent_only = NotificationEvaluationOutcome {
            permanent: permanent(),
            ..quiet()
        };
        assert_eq!(class_of(permanent_only), Some(FailureClass::Permanent));
    }

    #[test]
    fn a_server_error_is_recorded_as_one() {
        let outcome = NotificationEvaluationOutcome {
            server_error: Some(FailedAttempts::new(NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
                reason: EndpointRejection::HttpStatus {
                    status: 500,
                    retry_after: None,
                },
                class: FailureClass::ServerError,
            }))),
            ..quiet()
        };
        assert_eq!(
            disposition(outcome, 0, committed(), NewerDelivery::Absent),
            DeliveryDisposition::Failed {
                class: FailureClass::ServerError,
                detail: "1 endpoint attempt(s) were answered with a server error, the first because the endpoint answered HTTP 500".to_owned(),
                retry_after: None,
            }
        );
    }

    #[test]
    fn a_failure_is_settled_with_the_wait_its_endpoint_asked_for() {
        let mut unavailable = None;
        for retry_after in [Some(30), Some(90)] {
            FailedAttempts::count(
                &mut unavailable,
                NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
                    reason: EndpointRejection::HttpStatus {
                        status: 503,
                        retry_after: retry_after.map(Duration::from_secs),
                    },
                    class: FailureClass::Unavailable,
                }),
            );
        }
        let outcome = NotificationEvaluationOutcome { unavailable, ..quiet() };
        assert_eq!(
            disposition(outcome, 2, committed(), NewerDelivery::Absent),
            DeliveryDisposition::Failed {
                class: FailureClass::Unavailable,
                detail: "2 endpoint attempt(s) failed transiently, the first because the endpoint answered HTTP 503".to_owned(),
                retry_after: Some(Duration::from_secs(90)),
            }
        );
    }

    /// The instant the change of every delivery below committed at.
    fn committed() -> StoreInstant {
        StoreInstant::new(Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).single().expect("valid instant"))
    }

    /// `seconds` after the commit, negative for before it.
    fn after_commit(seconds: i64) -> DateTime<Utc> {
        committed().get() + TimeDelta::seconds(seconds)
    }

    /// The outcome of a notification the windows named withheld, both
    /// passing in 30 seconds.
    fn withheld_by(throttled_since: Option<DateTime<Utc>>, cooled_since: Option<DateTime<Utc>>) -> NotificationEvaluationOutcome {
        NotificationEvaluationOutcome {
            withheld: Some(Withholding {
                throttled_since,
                cooled_since,
                reopens_after: Duration::from_secs(30),
            }),
            ..quiet()
        }
    }

    /// A delivery deferred until the windows of [`withheld_by`] pass.
    const WAITS: DeliveryDisposition = DeliveryDisposition::Deferred {
        window: Duration::from_secs(30),
    };

    /// Table 5.2.15-1: "If requests are received before the cooldown period
    /// has expired, no notification is sent", so a change committed after
    /// the failure is withheld for good however the queue stands.
    #[test]
    fn a_first_attempt_whose_change_committed_inside_the_cooldown_is_withheld_for_good() {
        for failed_at in [after_commit(-1), after_commit(0)] {
            assert_eq!(
                disposition(withheld_by(None, Some(failed_at)), 0, committed(), NewerDelivery::Absent),
                DeliveryDisposition::Obsolete
            );
        }
    }

    /// A failure after the commit opened the cooldown while the delivery was
    /// queued, as when a probe is exhausted and hands its hold on, so the
    /// notification is owed once the cooldown has elapsed, newer deliveries
    /// or not.
    #[test]
    fn a_first_attempt_a_later_failure_cooled_waits_the_cooldown_out() {
        for newer in [NewerDelivery::Absent, NewerDelivery::Queued] {
            assert_eq!(disposition(withheld_by(None, Some(after_commit(5))), 0, committed(), newer), WAITS);
        }
    }

    /// Clause 5.2.12 lets one notification through per window: a newer
    /// delivery queued behind the withheld one notifies a later change, so
    /// the withheld one is dropped, while the newest waits the window out.
    #[test]
    fn a_throttled_first_attempt_is_superseded_by_a_newer_delivery_and_the_newest_waits() {
        let throttled = || withheld_by(Some(after_commit(5)), None);
        assert_eq!(disposition(throttled(), 0, committed(), NewerDelivery::Queued), DeliveryDisposition::Obsolete);
        assert_eq!(disposition(throttled(), 0, committed(), NewerDelivery::Absent), WAITS);
    }

    /// A newer delivery supersedes a throttled one whatever the cooldown
    /// says of it.
    #[test]
    fn a_newer_delivery_supersedes_a_throttled_one_a_later_failure_also_cooled() {
        assert_eq!(
            disposition(withheld_by(Some(after_commit(5)), Some(after_commit(5))), 0, committed(), NewerDelivery::Queued),
            DeliveryDisposition::Obsolete
        );
    }

    /// A retry was withheld by the window its own failed attempt opened, and
    /// the newer deliveries of its subscription wait behind it, so it waits
    /// the window out whatever else the windows say.
    #[test]
    fn a_withheld_retry_waits_the_window_out() {
        let outcome = || withheld_by(Some(after_commit(5)), Some(after_commit(-1)));
        assert_eq!(disposition(outcome(), 1, committed(), NewerDelivery::Queued), WAITS);
    }

    /// Stamps a racing claim moved explain no refusal, so nothing says the
    /// notification is not owed and it waits the whole window.
    #[test]
    fn a_refusal_no_stamp_explains_waits() {
        assert_eq!(disposition(withheld_by(None, None), 0, committed(), NewerDelivery::Queued), WAITS);
    }

    #[test]
    fn a_delivery_any_endpoint_accepted_is_delivered_and_one_owed_nothing_obsolete() {
        let delivered = NotificationEvaluationOutcome {
            delivered: 1,
            permanent: Some(FailedAttempts::new(NotificationAttemptFailure::NoData)),
            ..withheld_by(Some(after_commit(5)), None)
        };
        assert_eq!(disposition(delivered, 3, committed(), NewerDelivery::Absent), DeliveryDisposition::Delivered);
        assert_eq!(disposition(quiet(), 3, committed(), NewerDelivery::Absent), DeliveryDisposition::Obsolete);
    }
}
