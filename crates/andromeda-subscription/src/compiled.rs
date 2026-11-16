//! Pre-compiled, in-memory forms of subscriptions used by the matching hot path.
//!
//! These are derived artefacts, not stored state: `compile_subscription` builds
//! them once when a subscription enters the cache so that per-event matching
//! never re-parses a `q` expression, a `geoQ` geometry or a `scopeQ` pattern.
//! They live in subscription policy rather than the domain model because they
//! reference the query AST and the renderer's system-attribute policy. Transport
//! selection remains in delivery bindings and is not compiled here.

use crate::{
    entity_filter_set::EntityFilterSet,
    geo_filter::SubscriptionGeoFilter,
    notification_slot::NotificationCadence,
    notifying_since::NotifyingSince,
    subscription_query::SubscriptionQuery,
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_http_headers::language::LanguageFilter;
use andromeda_linked_retrieval::narrowing::list::ListInForce;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    positive_milliseconds::PositiveMilliseconds,
    spec_version::SpecVersion,
};
use andromeda_ngsi_ld_model::{
    entity_selector::EntitySelector,
    key_value_pair::KeyValuePair,
    notification::NotificationTrigger,
    notification_media_type::NotificationMediaType,
    subscription::standing::SubscriptionStanding,
};
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    dataset::DatasetSelector,
    entity_filters::EntityFilters,
    filter::{expression::Expr, reading::filter_reading::FilterReading},
    projection::compiled::Projection,
    scope::query::ScopeQuery,
};
use andromeda_ngsi_ld_query_evaluation::q::term_memo::TermInterner;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use std::{collections::HashSet, num::NonZeroU32};
use url::Url;

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    /// Subscription identifier.
    pub id: Identifier,
    /// The status and `expiresAt` deciding whether it is notified.
    pub standing: SubscriptionStanding,
    /// Notification endpoint URL.
    pub endpoint_uri: Url,
    /// The media type notifications are written as; absent means JSON.
    pub endpoint_accept: Option<NotificationMediaType>,
    /// Receiver metadata sent with a notification.
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    /// Transport-selection metadata interpreted by delivery bindings.
    pub endpoint_notifier_info: Option<Vec<KeyValuePair>>,
    /// Minimum period that must elapse after a failed delivery before this
    /// endpoint is addressed again (Table 5.2.15-1 `cooldown`).
    pub endpoint_cooldown: Option<PositiveMilliseconds>,
    /// Maximum period one delivery to this endpoint may take before it counts
    /// as failed (Table 5.2.15-1 `timeout`).
    pub endpoint_timeout: Option<PositiveMilliseconds>,
    /// Entity selectors applied to context-source information.
    pub selectors: Vec<EntitySelector>,
    /// The expanded attribute names matched against registrations, or every
    /// attribute when empty.
    ///
    /// Clauses 5.11.7 and 5.12: the combination of `watchedAttributes` and
    /// `notification.attributes`; when neither names an attribute, every
    /// attribute matches.
    pub matching_attrs: Vec<AttributeName>,
    /// The `temporalQ` the registrations' intervals are matched against
    /// (clause 5.11.2.4); absent means only latest-information registrations
    /// are considered.
    pub temporal_q: Option<CompiledCsrTemporalFilter>,
    /// The `geoQ` matched against the registration's geometry (clause 5.11.2.4).
    pub geo_q: SubscriptionGeoFilter,
    /// The `scopeQ` matched against the registration's `scope` (clause 5.11.2.4).
    pub scope_q: Option<ScopeQuery>,
    /// The `datasetId`s matched against the registration's `datasetId` (clause
    /// 5.12): a registration listing `datasetId`s is selected only when it
    /// shares one, and `None` selects every registration.
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Minimum seconds between notifications.
    pub throttling_secs: Option<f64>,
    /// Periodic notification interval in seconds.
    pub time_interval_secs: Option<f64>,
    /// JSON-LD context supplied with notifications.
    pub jsonld_context: Option<IriBuf>,
    /// The reading of the context-source filter, evaluated against the
    /// registration's Context Source Properties (clause 4.9).
    pub compiled_csf: Option<FilterReading>,
}

impl CompiledCsrSubscription {
    /// How this subscription comes to notify: on its `timeInterval` grid, or
    /// on change under its `throttling`.
    #[must_use]
    pub const fn cadence(&self) -> NotificationCadence {
        cadence(self.time_interval_secs, self.throttling_secs)
    }
}

/// The cadence a subscription's `timeInterval` and `throttling` members give
/// it; a `timeInterval` makes it periodic whatever `throttling` says, since
/// Table 5.2.12-1 does not let the two coexist.
const fn cadence(time_interval_secs: Option<f64>, throttling_secs: Option<f64>) -> NotificationCadence {
    match time_interval_secs {
        Some(_) => NotificationCadence::Periodic,
        None => NotificationCadence::OnChange { throttling_secs },
    }
}

/// Pre-compiled subscription for in-memory matching.
#[derive(Debug)]
pub struct CompiledSubscription {
    /// Subscription identifier.
    pub id: Identifier,
    /// The identity the store assigned this subscription, carried beside the
    /// identifier so a match enqueues its delivery against the narrow key while
    /// the notification it produces still names the subscription by its URI.
    pub key: SubscriptionKey,
    /// The status and `expiresAt` deciding whether it is notified.
    pub standing: SubscriptionStanding,
    /// Notification endpoint URL.
    pub endpoint_uri: Url,
    /// The media type notifications are written as; absent means JSON.
    pub endpoint_accept: Option<NotificationMediaType>,
    /// Receiver metadata sent with a notification.
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    /// Transport-selection metadata interpreted by delivery bindings.
    pub endpoint_notifier_info: Option<Vec<KeyValuePair>>,
    /// Minimum period that must elapse after a failed delivery before this
    /// endpoint is addressed again (Table 5.2.15-1 `cooldown`).
    pub endpoint_cooldown: Option<PositiveMilliseconds>,
    /// Maximum period one delivery to this endpoint may take before it counts
    /// as failed (Table 5.2.15-1 `timeout`).
    pub endpoint_timeout: Option<PositiveMilliseconds>,
    /// Events that trigger this subscription.
    pub triggers: Vec<NotificationTrigger>,
    /// Expanded watched attribute names, or every attribute when absent.
    pub watched_attrs: Option<HashSet<AttributeName>>,
    /// Entity selectors evaluated during matching.
    pub selectors: Vec<EntitySelector>,
    /// Minimum seconds between notifications.
    pub throttling_secs: Option<f64>,
    /// Periodic notification interval in seconds.
    pub time_interval_secs: Option<f64>,
    /// The clause 5.2.14 `notification.attributes`, the `pick` synonym, as
    /// expanded IRIs; empty when the subscription lists none.
    pub notification_attributes: Vec<IriBuf>,
    /// The clause 5.2.14 `pick` and `omit` members of the notification,
    /// compiled to the same projection a query's `pick`/`omit` parameters
    /// compile to (clause 4.21); empty on both sides when the subscription
    /// set neither.
    pub notification_projection: Projection,
    /// Requested notification representation format.
    pub notification_format: Option<ResponseFormat>,
    /// Whether system attributes remain in notification data.
    pub notification_sys_attrs: SysAttrsInclusion,
    /// Whether notification data contains previous values.
    pub notification_show_changes: bool,
    /// JSON-LD context supplied with notifications.
    pub jsonld_context: Option<IriBuf>,
    /// Since when the subscription may be notified of a committed change: a
    /// change the notification worker evaluates after the subscription was
    /// created, set active again or reselected is owed only if it committed
    /// after that.
    pub notifying_since: NotifyingSince,
    /// The entity filter, compiled under the language matching of the
    /// broker's `q_target_languages` level when the subscription was compiled,
    /// so a stored filter reads as the broker now reads every filter.
    pub compiled_q: Option<SubscriptionQuery>,
    /// The restriction the subscription's `geoQ` places on matching (clause
    /// 4.10): none without a `geoQ`, the compiled geoquery otherwise, and no
    /// entity at all for a stored `geoQ` that no longer compiles.
    pub compiled_geo_q: SubscriptionGeoFilter,
    /// The subscription's parsed `scopeQ`, if set.
    pub compiled_scope_q: Option<ScopeQuery>,
    /// The `datasetId` selectors of the Attribute instances each notification
    /// carries (clause 5.8.6), `@none` naming the default instance.
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Linked-entity join mode requested for notification data.
    pub notification_join: Option<JoinMode>,
    /// Maximum linked-entity join depth (the clause 4.5.23 joinLevel query parameter), which
    /// the spec bounds only by being a positive integer.
    pub notification_join_level: NonZeroU32,
    /// When true, this subscription only matches locally-stored entities (clause 5.5.13).
    pub local_only: bool,
    /// The NGSI-LD version the Entities of a notification are amended to
    /// conform to before they are sent (Table 5.2.12-1 `ngsildConformance`,
    /// clauses 5.8.6 and 4.3.6.8); `None` amends nothing.
    pub ngsild_conformance: Option<SpecVersion>,
    /// The clause 4.15 language filter every notified `LanguageProperty` is
    /// reduced by (Table 5.2.12-1 `lang`); `None` leaves the language maps
    /// whole.
    pub lang: Option<LanguageFilter>,
}

impl CompiledSubscription {
    /// The clause 4.21 list every entity this subscription notifies is
    /// narrowed by (clause 5.8.6): `pick` or `omit`, or else
    /// `notification.attributes` as the `pick` synonym.
    #[must_use]
    pub fn list_in_force(&self) -> ListInForce<'_> {
        ListInForce::of(Some(&self.notification_projection), &self.notification_attributes)
    }

    /// The `q`, `geoQ` and `scopeQ` an entity is filtered by (clause 5.8.6);
    /// a stored `geoQ` that no longer compiles leaves none to evaluate.
    #[must_use]
    pub const fn entity_filters(&self) -> EntityFilterSet<'_> {
        let geo = match &self.compiled_geo_q {
            SubscriptionGeoFilter::Unrestricted => None,
            SubscriptionGeoFilter::Restricts(query) => Some(query),
            SubscriptionGeoFilter::Unusable => return EntityFilterSet::AdmitsNothing,
        };
        EntityFilterSet::Evaluate(EntityFilters {
            q: match &self.compiled_q {
                Some(query) => Some(query.compiled()),
                None => None,
            },
            geo,
            scope: self.compiled_scope_q.as_ref(),
        })
    }

    /// Numbers the terms of the subscription's `q` with `interner`.
    pub fn number_terms(&mut self, interner: &TermInterner) {
        if let Some(query) = &mut self.compiled_q {
            query.number_terms(interner);
        }
    }

    /// The filters whose `rel{…}` hops cross into linked entities: the `q`.
    pub fn hopping_filters(&self) -> impl Iterator<Item = &Expr> {
        self.compiled_q
            .as_ref()
            .map(|query| query.compiled().expr())
            .into_iter()
            .filter(|filter| filter.contains_linked_hop())
    }

    /// Whether a filter of this subscription crosses into a linked entity
    /// (clause 4.9 `LinkedEntityRelation`), which only the entities the hop
    /// names, read from the store, can decide.
    #[must_use]
    pub fn filters_hop(&self) -> bool {
        self.hopping_filters().next().is_some()
    }

    /// How this subscription comes to notify: on its `timeInterval` grid, or
    /// on change under its `throttling`.
    #[must_use]
    pub const fn cadence(&self) -> NotificationCadence {
        cadence(self.time_interval_secs, self.throttling_secs)
    }

    /// Whether, as it stands at `now`, this subscription is notified of an
    /// entity change: it is active and unexpired (clause 5.8.6), and it has no
    /// `timeInterval`, which notifies on its own grid instead (Table 5.2.12-1).
    #[must_use]
    pub fn notifies_on_change_at(&self, now: DateTime<Utc>) -> bool {
        self.standing.notifies_at(now) && self.time_interval_secs.is_none()
    }
}

#[cfg(test)]
mod tests {
    use crate::{compiled::cadence, notification_slot::NotificationCadence};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_time_interval_makes_a_subscription_periodic() {
        assert_eq!(cadence(Some(10.0), None), NotificationCadence::Periodic);
    }

    #[test]
    fn a_subscription_without_a_time_interval_notifies_on_change_under_its_throttling() {
        assert_eq!(cadence(None, Some(5.0)), NotificationCadence::OnChange { throttling_secs: Some(5.0) });
        assert_eq!(cadence(None, None), NotificationCadence::OnChange { throttling_secs: None });
    }
}
