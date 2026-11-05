//! Entity event notification evaluation: which compiled subscriptions a
//! committed change reaches, and the delivery of one notification to each.

/// Why notifying one subscription failed, and the failed attempts of one change.
pub mod attempt_failure;
/// Who one committed change is evaluated for, and what its filters admit.
mod change_audience;
/// The entity state a change event is notified with.
mod change_payload;
/// Why a queued delivery is retried or failed.
mod delivery_failure;
/// Delivery of a durably queued event through the processor.
mod durable_delivery;
/// Which identifier a notification carries.
mod notification_identity;
pub(crate) mod payload;
/// Notifying one subscription of one committed entity change.
mod subscription_notification;

use crate::{
    error::NotificationError,
    outcome_batching::OutcomeBatcher,
    pending_change::{PendingChange, PendingCommits},
    processor::{
        attempt_failure::{FailedAttempts, NotificationAttemptFailure},
        change_audience::{ChangeAudience, addressed, admitted_by_entity_filters, changed_states, in_force_versions, owed_since_commit},
        change_payload::notified_state,
        notification_identity::NotificationIdentity,
        payload::cache::NotificationCompactionCache,
        subscription_notification::NotifiedChange,
    },
};
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    endpoint::{
        error::EndpointParseError,
        transport::{EndpointTransport, parse_endpoint_transport},
    },
};
use andromeda_entity::{error::EntityError, port::reader::EntityReader, query::EntityQueryService};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_entity::queried::linked_states::LinkedStates;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::filter::expression::Expr;
use andromeda_retry::retry_delay::RetryDelay;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change_filters::ChangedStates,
    compiled::CompiledSubscription,
    error::SubscriptionError,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use futures::{StreamExt, stream};
use std::sync::Arc;
use tracing::{debug, warn};

const NOTIFICATION_CONCURRENCY: usize = 20;

fn compiled_transport(subscription: &CompiledSubscription) -> Result<EndpointTransport, EndpointParseError> {
    parse_endpoint_transport(&subscription.endpoint_uri, subscription.endpoint_notifier_info.as_deref())
        .inspect_err(|error| debug!(subscription = %subscription.id, %error, "Compiled subscription endpoint is no longer deliverable"))
}

/// Fold the attempts of one committed change into its aggregate outcome; a
/// subscription whose throttling window swallowed the notification
/// contributes nothing.
fn aggregate_delivery_outcomes(attempts: Vec<Option<Result<(), NotificationAttemptFailure>>>) -> NotificationEvaluationOutcome {
    attempts
        .into_iter()
        .flatten()
        .fold(NotificationEvaluationOutcome::default(), |mut outcome, attempt| {
            match attempt {
                Ok(()) => outcome.delivered += 1,
                Err(failure) if failure.is_retryable() => FailedAttempts::count(&mut outcome.retryable, failure),
                Err(failure) => FailedAttempts::count(&mut outcome.terminal, failure),
            }
            outcome
        })
}

/// Applies subscription and entity changes to entity-notification policy.
pub struct NotificationProcessor {
    live_target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    outcomes: OutcomeBatcher,
    entity_reader: Arc<dyn EntityReader>,
    entity_query: Arc<EntityQueryService>,
    cache: Arc<SubscriptionCache>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: TenantRef,
    pending_commits: Arc<PendingCommits>,
}

#[derive(Clone)]
/// Focused capabilities used by entity-notification evaluation.
pub struct NotificationProcessorDeps {
    /// Tenant live branch loaded during worker startup.
    pub live_target: ContextTarget,
    /// Entity-subscription persistence.
    pub sub_repo: Arc<dyn SubscriptionStore>,
    /// Current entity reads used to assemble notification data.
    pub entity_reader: Arc<dyn EntityReader>,
    /// Linked-entity query policy used by join projection.
    pub entity_query: Arc<EntityQueryService>,
    /// Tenant JSON-LD processor.
    pub engine: JsonLdEngine,
    /// Selected notification delivery capability.
    pub transport: Arc<TransportDispatcher>,
    /// Tenant partition for spatial matching and delivery metadata.
    pub tenant: TenantRef,
    /// Compiled subscription matching state.
    pub subscription_cache: Arc<SubscriptionCache>,
    /// The changes handed to live evaluation and not yet evaluated, which
    /// bound how long a superseded selection is kept.
    pub pending_commits: Arc<PendingCommits>,
}

/// Aggregate endpoint outcomes from evaluating one committed entity change.
#[derive(Debug, Default)]
pub struct NotificationEvaluationOutcome {
    /// Endpoints that accepted a notification.
    pub delivered: usize,
    /// Failures worth retrying, `None` when there were none.
    pub retryable: Option<FailedAttempts>,
    /// Endpoint or payload failures that retry cannot repair, `None` when
    /// there were none.
    pub terminal: Option<FailedAttempts>,
}

impl NotificationProcessor {
    /// Construct event evaluation from its focused capabilities, inside a
    /// Tokio runtime: the task recording its notification outcomes runs on it.
    #[must_use]
    pub fn new(deps: NotificationProcessorDeps) -> NotificationProcessor {
        NotificationProcessor {
            live_target: deps.live_target,
            outcomes: OutcomeBatcher::new(Arc::clone(&deps.sub_repo)),
            sub_repo: deps.sub_repo,
            entity_reader: deps.entity_reader,
            entity_query: deps.entity_query,
            cache: deps.subscription_cache,
            transport: deps.transport,
            engine: deps.engine,
            tenant: deps.tenant,
            pending_commits: deps.pending_commits,
        }
    }

    /// Reload compiled subscriptions after startup or an event-stream gap.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store
    /// cannot list the tenant's subscriptions; the cache is then left as it was.
    pub async fn reload_cache(&self) -> Result<(), NotificationError> {
        self.cache.reload(&self.live_target, self.sub_repo.as_ref()).await.map_err(Into::into)
    }

    /// Read again the subscriptions whose store the cache could not reach
    /// when they changed, retrying on `delay`.
    pub async fn reconcile_subscriptions(&self, delay: &dyn RetryDelay) {
        self.cache.reconcile_dirty(self.sub_repo.as_ref(), delay).await;
    }

    /// Evaluate and deliver notifications for one committed entity change,
    /// then stop holding it.
    pub async fn apply_entity_change(&self, pending: PendingChange) -> NotificationEvaluationOutcome {
        let PendingChange { change, hold } = pending;
        let ContextChangeEvent { target, event, committed_at } = change;
        let outcome = self
            .apply_entity_change_for(target, event, committed_at, ChangeAudience::Live, NotificationIdentity::Live)
            .await;
        drop(hold);
        self.release_superseded();
        outcome
    }

    /// Drop the superseded selections no change still held for evaluation can
    /// have committed under.
    ///
    /// Subscription changes reach the cache through the change sink that
    /// published them, so the processor only releases what they superseded.
    pub fn release_superseded(&self) {
        self.cache.release_superseded(self.pending_commits.floor());
    }

    async fn apply_entity_change_for(
        &self,
        target: ContextTarget,
        event: ChangeEvent,
        committed_at: StoreInstant,
        audience: ChangeAudience<'_>,
        identity: NotificationIdentity,
    ) -> NotificationEvaluationOutcome {
        debug!(
            entity = %event.entity_id,
            lifecycle = ?event.change.lifecycle,
            types = ?event.entity_types,
            attributes = ?event.change.attributes(),
            "Change event received"
        );
        let candidates = self.candidate_subscriptions(&target, &event, committed_at, audience);
        if candidates.is_empty() {
            debug!(entity = %event.entity_id, "No matching subscriptions for the change");
            return NotificationEvaluationOutcome::default();
        }
        debug!("Found {} matching subscriptions for event {}", candidates.len(), event.entity_id);

        // The entity is read once and shared by every notification the change produces.
        let notified = notified_state(self.entity_reader.as_ref(), &target, &event).await;
        let states = changed_states(&event, notified.as_deref());
        let linked = match self.linked_states_for(&target, &event, &candidates, states).await {
            Ok(linked) => linked,
            // The hops cannot be decided without their linked entities; a
            // queued delivery is retried, as for any read that may answer the
            // next attempt.
            Err(error) => {
                return NotificationEvaluationOutcome {
                    retryable: Some(FailedAttempts::new(NotificationAttemptFailure::LinkedEntitiesUnread(error))),
                    ..NotificationEvaluationOutcome::default()
                };
            }
        };
        let matching = in_force_versions(
            &self.cache,
            &target,
            admitted_by_entity_filters(audience, &self.cache, &target, &event, candidates, states, &linked),
        );
        if matching.is_empty() {
            return NotificationEvaluationOutcome::default();
        }

        let compaction_cache = NotificationCompactionCache::default();
        let change = NotifiedChange {
            target: &target,
            event: &event,
            notified: notified.as_ref(),
            now: Utc::now(),
            compaction_cache: &compaction_cache,
            identity,
        };
        let attempts: Vec<Option<Result<(), NotificationAttemptFailure>>> = stream::iter(matching)
            .map(|compiled_sub| {
                let change = &change;
                async move { self.notify_subscription(&compiled_sub, change).await }
            })
            .buffer_unordered(NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
        aggregate_delivery_outcomes(attempts)
    }

    /// Re-read one subscription from the store before the change is matched
    /// against it.
    ///
    /// A durably queued delivery may be drained by another instance of this
    /// broker, which shares the store but not the in-memory cache the
    /// subscription's own lifecycle changes refreshed. Reconciling first means
    /// a stale cache neither drops a subscription created elsewhere nor
    /// notifies one deleted there (clause 5.8.6).
    ///
    /// # Errors
    ///
    /// Returns the store's failure when the subscription cannot be read back;
    /// the cache is then left as it was.
    async fn refresh_cached_subscription(&self, target: &ContextTarget, subscription_id: &Identifier) -> Result<(), SubscriptionError> {
        let refreshed = self.cache.refresh(target, subscription_id, self.sub_repo.as_ref()).await;
        self.release_superseded();
        refreshed
    }

    /// The compiled subscriptions the change committed at `committed_at` is
    /// owed to before entity filters are evaluated: every one it reaches for a
    /// live change, under the selection that stood at its commit, the one a
    /// durably queued delivery is addressed to.
    ///
    /// The subscription's own state is read as it stands now, so a subscription
    /// deleted, paused or expired since the change was queued is not notified
    /// of it, while nothing here reads the entity. Clause 5.8.6 notifies only
    /// an active subscription, so one that became active after the change
    /// committed was paused, expired or absent when it happened and is owed
    /// nothing for it; and one whose selection changed after the commit is
    /// judged by the superseded selection while the cache keeps it, and owed
    /// nothing once it does not.
    fn candidate_subscriptions(
        &self,
        target: &ContextTarget,
        event: &ChangeEvent,
        committed_at: StoreInstant,
        audience: ChangeAudience<'_>,
    ) -> Vec<Arc<CompiledSubscription>> {
        owed_since_commit(audience, committed_at, addressed(audience, &self.cache, target, event, committed_at))
    }

    /// The entities the `rel{…}` hops of the candidates' filters cross into
    /// (clause 4.9), read from the store as they stand now; none when no
    /// candidate's filters hop.
    ///
    /// # Errors
    ///
    /// Returns the store's failure when a level of linked entities cannot be
    /// read.
    async fn linked_states_for(
        &self,
        target: &ContextTarget,
        event: &ChangeEvent,
        candidates: &[Arc<CompiledSubscription>],
        states: ChangedStates<'_>,
    ) -> Result<LinkedStates, EntityError> {
        let filters: Vec<&Expr> = candidates.iter().flat_map(|candidate| candidate.hopping_filters()).collect();
        if filters.is_empty() {
            return Ok(LinkedStates::new());
        }
        let roots = states.filtered(&event.change);
        self.entity_query
            .linked_filter_states(target, &roots, &filters, Utc::now())
            .await
            .inspect_err(|error| warn!(entity = %event.entity_id, %error, "the entities a subscription filter links to could not be read"))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::processor::change_audience::{ChangeAudience, CommitFilters, addressed, admitted_by_entity_filters, in_force_versions, owed_since_commit};
    use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        queried::linked_states::LinkedStates,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
        subscription::{SubscriptionStatus, entity_selector::EntitySelector, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_query::{
        filter::{language_matching::LanguageMatching, parser},
        projection::compiled::Projection,
        queried_entity::linked_entities::NoLinkedEntities,
        scope::query::ScopeQuery,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_subscription::{
        cache::SubscriptionCache,
        change_filters::ChangedStates,
        compiled::CompiledSubscription,
        geo_filter::SubscriptionGeoFilter,
        matching::{matches_entity_filters, matches_subscription},
        notifying_since::NotifyingSince,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use std::{num::NonZeroU32, sync::Arc};
    use url::Url;

    /// A modification that `kind`s the default instance of every attribute in
    /// `attrs`.
    fn make_event(entity_id: &str, types: Vec<&str>, kind: AttributeChangeKind, attrs: Vec<&str>) -> ChangeEvent {
        let instances = attrs
            .into_iter()
            .map(|attribute| AttributeChange {
                key: AttributeInstanceKey {
                    attr: attribute.parse().expect("valid test attribute name"),
                    dataset_id: None,
                },
                kind,
            })
            .collect();
        ChangeEvent::builder()
            .entity_id(entity_id.parse().expect("valid test URI"))
            .entity_types(types.into_iter().map(|t| t.parse().expect("valid test type name")).collect())
            .change(EntityChange::new(EntityLifecycle::Modified, instances))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    fn make_sub(id: &str, selectors: Vec<EntitySelector>, triggers: Vec<&str>, watched: Option<Vec<&str>>) -> CompiledSubscription {
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}").parse().expect("a valid subscription URI"),
            key: SubscriptionKey::try_from(1).expect("a valid subscription key"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("a valid endpoint URI"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: triggers.into_iter().map(|t| t.parse().expect("valid trigger")).collect(),
            watched_attrs: watched.map(|attributes| {
                attributes
                    .into_iter()
                    .map(|attribute| attribute.parse().expect("valid test attribute name"))
                    .collect()
            }),
            selectors,
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
            compiled_q: None,
            language_matching: LanguageMatching::TagEquality,
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: Projection::default(),
        }
    }

    #[test]
    fn matches_by_type_and_trigger() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_type() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Parking".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_trigger() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["entityCreated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_watched_attributes() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["Vehicle"],
            AttributeChangeKind::Updated,
            vec!["speed", "location"],
        );
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_unwatched_attribute() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["location"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn an_id_takes_precedence_over_a_non_matching_id_pattern() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: Some("urn:ngsi-ld:Vehicle:A1".parse().expect("valid id")),
                id_pattern: Some("urn:ngsi-ld:Parking:.*".parse().expect("valid pattern")),
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_with_id_pattern() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: Some("urn:ngsi-ld:Vehicle:.*".parse().expect("valid pattern")),
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_id_pattern_mismatch() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: Some("urn:ngsi-ld:Parking:.*".parse().expect("valid pattern")),
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_specific_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: Some("urn:ngsi-ld:Vehicle:A1".parse().expect("valid uri")),
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_different_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: Some("urn:ngsi-ld:Vehicle:B2".parse().expect("valid uri")),
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn expired_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.standing.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_selectors_matches_all_entities() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn paused_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.standing.status = SubscriptionStatus::Paused;
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    fn at(unix_seconds: i64) -> StoreInstant {
        StoreInstant::new(DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range"))
    }

    /// A Vehicle subscription active since `active` and selecting as it does
    /// since `selected`.
    fn vehicle_sub_since(active: i64, selected: i64) -> Arc<CompiledSubscription> {
        let mut sub = make_sub("since", vec![], vec!["attributeUpdated"], None);
        sub.notifying_since = NotifyingSince {
            active: at(active),
            selected: at(selected),
        };
        Arc::new(sub)
    }

    fn owed_ids(audience: ChangeAudience<'_>, committed_at: StoreInstant, candidates: Vec<Arc<CompiledSubscription>>) -> usize {
        owed_since_commit(audience, committed_at, candidates).len()
    }

    /// Table 5.2.12-1 `isActive` and clause 5.8.6: the Subscription was paused
    /// when the change committed at 1 500 and set active again at 2 000; the
    /// change is evaluated only now, after the reactivation, and still owes
    /// nothing.
    #[test]
    fn a_change_committed_while_paused_and_evaluated_after_reactivation_sends_nothing() {
        pretty_assertions::assert_eq!(owed_ids(ChangeAudience::Live, at(1_500), vec![vehicle_sub_since(2_000, 1_000)]), 0);
    }

    #[test]
    fn a_change_committed_after_the_subscription_was_set_active_is_notified() {
        pretty_assertions::assert_eq!(owed_ids(ChangeAudience::Live, at(2_500), vec![vehicle_sub_since(2_000, 1_000)]), 1);
    }

    /// Clause 5.2.12 has no initial notification: a Subscription created at
    /// 2 000 is owed nothing for a change committed at 1 000, however late it
    /// is evaluated.
    #[test]
    fn a_change_committed_before_the_subscription_was_created_is_not_notified_to_it() {
        pretty_assertions::assert_eq!(owed_ids(ChangeAudience::Live, at(1_000), vec![vehicle_sub_since(2_000, 2_000)]), 0);
    }

    /// The selection changed at 2 000, after the change committed at 1 500.
    /// A live change is judged now, by a selection that did not exist when it
    /// happened; a queued delivery was matched by the commit, under the
    /// selection of its commit, unless the commit left its filters undecided.
    #[test]
    fn a_selection_changed_after_the_commit_drops_only_what_is_judged_after_it() {
        let subscription = vehicle_sub_since(1_000, 2_000);
        let id = subscription.id.clone(); // clone: the audience borrows the identifier the candidate list consumes
        pretty_assertions::assert_eq!(owed_ids(ChangeAudience::Live, at(1_500), vec![Arc::clone(&subscription)]), 0);
        let decided = ChangeAudience::Queued {
            subscription: &id,
            filters: CommitFilters::Decided,
        };
        pretty_assertions::assert_eq!(owed_ids(decided, at(1_500), vec![Arc::clone(&subscription)]), 1);
        let undecided = ChangeAudience::Queued {
            subscription: &id,
            filters: CommitFilters::Undecided,
        };
        pretty_assertions::assert_eq!(owed_ids(undecided, at(1_500), vec![subscription]), 0);
    }

    /// A queued delivery is owed only while the Subscription has been active
    /// since its commit: one paused and set active again in between is not.
    #[test]
    fn a_queued_delivery_reactivated_after_its_commit_is_not_sent() {
        let subscription = vehicle_sub_since(2_000, 1_000);
        let id = subscription.id.clone(); // clone: the audience borrows the identifier the candidate list consumes
        let decided = ChangeAudience::Queued {
            subscription: &id,
            filters: CommitFilters::Decided,
        };
        pretty_assertions::assert_eq!(owed_ids(decided, at(1_500), vec![subscription]), 0);
    }

    /// A Truck subscription watching `heading`, which a Vehicle `speed` update
    /// does not reach.
    fn truck_heading_sub(id: &str) -> CompiledSubscription {
        make_sub(
            id,
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Truck".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            Some(vec!["heading"]),
        )
    }

    fn addressed_ids(audience: ChangeAudience<'_>, cache: &SubscriptionCache, event: &ChangeEvent) -> Vec<String> {
        addressed(audience, cache, &ContextTarget::Live, event, StoreInstant::new(Utc::now()))
            .iter()
            .map(|candidate| candidate.id.to_string())
            .collect()
    }

    /// Clauses 5.8.2 and 5.8.6: the commit queued the `speed` update for the
    /// Subscription while it selected Vehicles and watched `speed`; an update
    /// to Trucks and `heading` before delivery changes what later writes
    /// notify, not the notification already owed.
    #[test]
    fn a_queued_delivery_is_addressed_to_its_subscription_however_its_selection_moved_since() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let moved = truck_heading_sub("moved");
        let id = moved.id.clone(); // clone: the audience borrows the identifier the cache takes
        cache.insert(&ContextTarget::Live, moved);
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        pretty_assertions::assert_eq!(addressed_ids(ChangeAudience::Live, &cache, &event), Vec::<String>::new());
        for filters in [CommitFilters::Decided, CommitFilters::Undecided] {
            let queued = ChangeAudience::Queued { subscription: &id, filters };
            pretty_assertions::assert_eq!(addressed_ids(queued, &cache, &event), vec!["urn:ngsi-ld:Subscription:moved".to_owned()]);
        }
    }

    /// Clause 5.8.6 notifies only an active Subscription, and Table 5.2.12-1
    /// sends a periodic one its notifications on its own grid: a queued
    /// delivery whose Subscription was paused, turned periodic or deleted
    /// before delivery is addressed to nobody.
    #[test]
    fn a_queued_delivery_addresses_no_subscription_paused_periodic_or_gone() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let mut paused = truck_heading_sub("paused");
        paused.standing.status = SubscriptionStatus::Paused;
        let mut periodic = truck_heading_sub("periodic");
        periodic.key = SubscriptionKey::try_from(2).expect("a valid subscription key");
        periodic.time_interval_secs = Some(10.0);
        let ids: Vec<Identifier> = [&paused, &periodic].iter().map(|candidate| candidate.id.clone()).collect(); // clone: the audiences borrow the identifiers the cache takes
        cache.insert(&ContextTarget::Live, paused);
        cache.insert(&ContextTarget::Live, periodic);
        let gone: Identifier = "urn:ngsi-ld:Subscription:gone".parse().expect("a valid subscription URI");
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        for id in ids.iter().chain([&gone]) {
            let queued = ChangeAudience::Queued {
                subscription: id,
                filters: CommitFilters::Decided,
            };
            pretty_assertions::assert_eq!(addressed_ids(queued, &cache, &event), Vec::<String>::new(), "{id}");
        }
    }

    /// A Vehicle subscription watching `attribute` since `selected`.
    fn vehicle_watching_since(attribute: &str, selected: i64) -> CompiledSubscription {
        let mut sub = make_sub("reselected", vec![], vec!["attributeUpdated"], Some(vec![attribute]));
        sub.notifying_since = NotifyingSince {
            active: at(0),
            selected: at(selected),
        };
        sub
    }

    /// Clause 5.8.6: the `speed` update committed at 1 500, while `speed` was
    /// watched, is owed though the Subscription watches `heading` since 2 000
    /// by the time the change is evaluated; it is sent as the Subscription now
    /// stands.
    #[test]
    fn a_live_change_committed_before_a_reselection_is_owed_under_the_selection_of_its_commit() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        cache.insert(&ContextTarget::Live, vehicle_watching_since("speed", 1_000));
        cache.insert(&ContextTarget::Live, vehicle_watching_since("heading", 2_000));
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        let watched = |candidates: &[Arc<CompiledSubscription>]| -> Vec<String> {
            candidates
                .iter()
                .flat_map(|candidate| candidate.watched_attrs.iter().flatten().map(ToString::to_string))
                .collect()
        };

        let owed = owed_since_commit(
            ChangeAudience::Live,
            at(1_500),
            addressed(ChangeAudience::Live, &cache, &ContextTarget::Live, &event, at(1_500)),
        );
        pretty_assertions::assert_eq!(watched(&owed), vec!["speed".to_owned()]);
        pretty_assertions::assert_eq!(watched(&in_force_versions(&cache, &ContextTarget::Live, owed)), vec!["heading".to_owned()]);

        let later = owed_since_commit(
            ChangeAudience::Live,
            at(2_500),
            addressed(ChangeAudience::Live, &cache, &ContextTarget::Live, &event, at(2_500)),
        );
        pretty_assertions::assert_eq!(later.len(), 0, "a speed update after the reselection is not owed");
    }

    /// Clause 5.8.6 sends nothing once the Subscription is gone, whichever
    /// selection the change was matched under.
    #[test]
    fn a_matched_subscription_deleted_before_it_is_notified_is_dropped() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let matched = Arc::new(vehicle_watching_since("speed", 1_000));
        pretty_assertions::assert_eq!(in_force_versions(&cache, &ContextTarget::Live, vec![matched]).len(), 0);
    }

    /// The states a modification leaving `state` is filtered against.
    const fn current(state: &EntityState) -> ChangedStates<'_> {
        ChangedStates {
            current: Some(state),
            previous: None,
        }
    }

    /// A Vehicle with a `speed` of 80, a `location` and two scopes.
    fn entity_with_geo_and_scope() -> EntityState {
        let document = serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]
            }],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 8.684_783}, {"@value": 49.406_132}]}]
                }]
            }],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid/Gardens"}, {"@value": "/Spain"}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    #[test]
    fn matches_entity_filters_q_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">50").expect("valid q"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_q_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">100").expect("valid q"));
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_scope_exact() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/Spain").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_scope_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/France").expect("valid scope query"));
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_scope_hash_wildcard() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/Madrid/#").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_scope_and() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("(/Spain;/Madrid/Gardens)").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_scope_or() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/France,/Spain").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_geo_near() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let stored = serde_json::from_value(serde_json::json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}))
            .expect("a readable geoQ");
        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(Some(&stored), "test");
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_geo_near_too_far() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let stored = serde_json::from_value(serde_json::json!({"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}))
            .expect("a readable geoQ");
        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(Some(&stored), "test");
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    #[test]
    fn matches_entity_filters_no_filters() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope(), NoLinkedEntities));
    }

    /// Design: the commit matched a durable delivery's `q`, `geoQ`, `scopeQ`
    /// and `csf` on the state queued with it, so delivery does not evaluate
    /// them again; a live change is filtered now.
    #[test]
    fn durable_delivery_does_not_re_evaluate_a_matched_filter() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">100").expect("valid q"));
        let subscription_id = sub.id.clone(); // clone: the audience names the subscription the candidates own
        let candidates = vec![Arc::new(sub)];
        let state = entity_with_geo_and_scope();
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["https://example.com/Vehicle"],
            AttributeChangeKind::Updated,
            vec!["https://example.com/speed"],
        );

        // clone: the same candidates are offered to both audiences
        let live = admitted_by_entity_filters(
            ChangeAudience::Live,
            &cache,
            &ContextTarget::Live,
            &event,
            candidates.clone(),
            current(&state),
            &LinkedStates::new(),
        );
        assert!(live.is_empty(), "a live change is filtered by the q it fails");

        let decided = ChangeAudience::Queued {
            subscription: &subscription_id,
            filters: CommitFilters::Decided,
        };
        // clone: same
        let queued = admitted_by_entity_filters(
            decided,
            &cache,
            &ContextTarget::Live,
            &event,
            candidates.clone(),
            current(&state),
            &LinkedStates::new(),
        );
        pretty_assertions::assert_eq!(queued.iter().map(|sub| &sub.id).collect::<Vec<_>>(), vec![&subscription_id]);

        // A commit that had no state to read admitted the change undecided, so
        // its delivery decides the filters on the entity it reads.
        let undecided = ChangeAudience::Queued {
            subscription: &subscription_id,
            filters: CommitFilters::Undecided,
        };
        assert!(
            admitted_by_entity_filters(
                undecided,
                &cache,
                &ContextTarget::Live,
                &event,
                candidates,
                current(&state),
                &LinkedStates::new()
            )
            .is_empty()
        );
    }

    /// The commit cannot read the entities a `rel{…}` hop crosses into, so it
    /// recorded a hopping subscription undecided: its delivery decides the hop
    /// on the linked entities it reads, even for a delivery whose other
    /// filters the commit decided.
    #[test]
    fn a_queued_delivery_decided_at_commit_still_decides_a_hopping_filter() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/isParkedIn\"{\"https://example.com/category\"}==\"commercial\"").expect("valid q"));
        let subscription_id = sub.id.clone(); // clone: the audience names the subscription the candidates own
        let candidates = vec![Arc::new(sub)];
        let vehicle = EntityState::from_slice(
            &serde_json::to_vec(&serde_json::json!([{
                "@id": "urn:ngsi-ld:Vehicle:A1",
                "@type": ["https://example.com/Vehicle"],
                "https://example.com/isParkedIn": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                    "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Parking:1"}]
                }]
            }]))
            .expect("JSON"),
        )
        .expect("a stored entity decodes");
        let parking = |category: &str| {
            let mut linked = LinkedStates::new();
            linked.insert(Arc::new(
                EntityState::from_slice(
                    &serde_json::to_vec(&serde_json::json!([{
                        "@id": "urn:ngsi-ld:Parking:1",
                        "@type": ["https://example.com/Parking"],
                        "https://example.com/category": [{
                            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]
                        }]
                    }]))
                    .expect("JSON"),
                )
                .expect("a stored entity decodes"),
            ));
            linked
        };
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["https://example.com/Vehicle"],
            AttributeChangeKind::Updated,
            vec!["https://example.com/isParkedIn"],
        );
        let decided = ChangeAudience::Queued {
            subscription: &subscription_id,
            filters: CommitFilters::Decided,
        };
        // clone: the same candidates are offered to both readings
        let residential = admitted_by_entity_filters(
            decided,
            &cache,
            &ContextTarget::Live,
            &event,
            candidates.clone(),
            current(&vehicle),
            &parking("residential"),
        );
        assert!(residential.is_empty(), "the hop is decided at delivery, and fails");
        let commercial = admitted_by_entity_filters(
            decided,
            &cache,
            &ContextTarget::Live,
            &event,
            candidates,
            current(&vehicle),
            &parking("commercial"),
        );
        pretty_assertions::assert_eq!(commercial.iter().map(|sub| &sub.id).collect::<Vec<_>>(), vec![&subscription_id]);
    }
}
