//! Reducing a subscription to what one registration matches (clause 5.8.1.4).
//!
//! The copy placed at a context source watches only the entities and attributes
//! the registration declared and notifies this broker rather than the
//! subscriber. Its entity selectors are the subscription's intersected with
//! the registered entities, so no type or identifier outside the registration
//! reaches the source (clause 4.3.6.1). The `attributes`, `pick` and `omit` of
//! the notification member never travel with it: this broker narrows the
//! entities it re-assembles before delivering them (clause 5.8.6), and a
//! source that had already narrowed its part would leave nothing to
//! re-assemble from. The filters `q`, `geoQ` and `scopeQ` go only when
//! entities may be split, because only then can they not be decided at one
//! source. The `q` and the `csf` are spelled in the terms of the `@context` the
//! copy is written under, from the expanded filter the subscription stores: a
//! context source expands them with that `@context`, so each name reaches the
//! IRI this broker matches, and a member that reads as a language tag stays the
//! tag. The context source filter `csf` goes as the subscription holds it
//! whatever the carriage: it decides no entity, only the further sources the
//! copy reaches from the source it is placed at (Table 5.2.12-1), and clause
//! 5.8.1.4 reduces no member but those. The `datasetId` names only the values
//! the registration lists, and every value it lists when the subscription
//! names none, since the source provides no other instance (clause 4.3.6.1,
//! Table 5.2.9-1); a registration listing none is sent the subscription's own.
//! A periodic copy watches nothing, since Table 5.2.12-1 gives
//! `watchedAttributes` no place beside `timeInterval`, so no member of it
//! narrows the Attributes its source notifies, and this broker keeps only the
//! Attributes the registration covers of what the source sends (clause
//! 4.3.6.1).

use crate::receiver::ReceiverEndpoint;
use andromeda_federation::split_entities::SplitEntities;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    notification_request::NotificationRequest,
    registration::ContextSourceRegistration,
    registration_matching::selector_reduction::reduced_selectors,
    subscription::{Subscription, SubscriptionType},
    subscription_request::SubscriptionRequest,
    written_subscription_terms::{WrittenSubscriptionTerms, WrittenTermsError},
};
use andromeda_ngsi_ld_query::{
    dataset_selector::DatasetSelector,
    filter::{
        Filter,
        compaction::{compact_context_source_filter, compact_filter},
    },
    name_stage::Written,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_subscription::{
    deleted_members::{DeletableMember, DeletedMembers},
    error::SubscriptionError,
    subscription_invariant::SubscriptionInvariant,
};

/// Whether the copy keeps the filters of the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterCarriage {
    /// Entities are complete at every source: the filters travel with the copy.
    Keep,
    /// Entities may be split: the filters are applied after re-assembly only.
    Strip,
}

impl FilterCarriage {
    /// The carriage clause 5.8.1.4 prescribes for a resolved `splitEntities`.
    #[must_use]
    pub const fn for_split(split_entities: SplitEntities) -> FilterCarriage {
        match split_entities {
            SplitEntities::Split => FilterCarriage::Strip,
            SplitEntities::Complete => FilterCarriage::Keep,
        }
    }
}

/// The attribute names a registration declares across every block, or `None`
/// when some block declares none (it then covers every attribute).
fn declared_attributes(registration: &ContextSourceRegistration) -> Option<Vec<&AttributeName>> {
    let mut declared = Vec::new();
    for block in &registration.coverage.information {
        let mut names = block.attribute_names().peekable();
        names.peek()?;
        declared.extend(names);
    }
    Some(declared)
}

/// Narrow a name list to the names the registration declares; `None` on
/// either side means "every attribute" and yields the other side unchanged.
fn narrow_names(names: Option<&[AttributeName]>, declared: Option<&[&AttributeName]>) -> Option<Vec<AttributeName>> {
    match (names, declared) {
        (None, None) => None,
        (None, Some(declared)) => Some(declared.iter().map(|name| (*name).clone()).collect()),
        (Some(names), None) => Some(names.to_vec()),
        (Some(names), Some(declared)) => Some(names.iter().filter(|name| declared.contains(name)).cloned().collect()),
    }
}

/// The `watchedAttributes` of the copy: the subscription's narrowed to the
/// names the registration declares, and none for a periodic subscription
/// (Table 5.2.12-1).
fn copy_watched_attributes(subscription: &Subscription, watched: Option<&[AttributeName]>, declared: Option<&[&AttributeName]>) -> Option<Vec<AttributeName>> {
    match subscription.delivery.time_interval {
        Some(_) => None,
        None => narrow_names(watched, declared),
    }
}

/// The `datasetId`s the copy placed at `registration`'s source names (clause
/// 4.3.6.1): the subscription's that the registration lists, in the
/// subscription's order, every one it lists when the subscription names none,
/// and the subscription's own when it lists none; `None` when that leaves no
/// value.
fn copy_dataset_ids(subscription: &Subscription, registration: &ContextSourceRegistration) -> Option<Vec<DatasetSelector>> {
    let named: Vec<DatasetSelector> = registration
        .coverage
        .dataset_overlap(subscription.filters.dataset_id.as_deref())
        .forwarded()
        .cloned()
        .collect();
    (!named.is_empty()).then_some(named)
}

/// The `q` of `subscription` spelled in the terms of `names`, when the copy keeps the filters.
///
/// # Errors
///
/// Returns [`WrittenTermsError::Compaction`] when a guard type has no written name.
async fn copy_q(subscription: &Subscription, carriage: FilterCarriage, names: &NameCompaction) -> Result<Option<Filter<Written>>, WrittenTermsError> {
    match (&subscription.filters.q, carriage) {
        (Some(q), FilterCarriage::Keep) => Ok(Some(compact_filter(q.clone(), names).await.map_err(Box::new)?)), // clone: compaction consumes the tree and the subscription keeps its own
        (Some(_), FilterCarriage::Strip) | (None, FilterCarriage::Keep | FilterCarriage::Strip) => Ok(None),
    }
}

/// The `csf` of `subscription` spelled in the terms of `names`.
///
/// # Errors
///
/// As [`copy_q`].
async fn copy_csf(subscription: &Subscription, names: &NameCompaction) -> Result<Option<Filter<Written>>, WrittenTermsError> {
    match &subscription.filters.csf {
        Some(csf) => Ok(Some(
            compact_context_source_filter(csf.clone(), names).await.map_err(Box::new)?, // clone: compaction consumes the tree and the subscription keeps its own
        )),
        None => Ok(None),
    }
}

/// The reduced subscription placed at a context source.
#[derive(Debug, Clone)]
pub struct ReducedSubscription {
    /// The create body placed at the source, with expanded IRIs; the
    /// transport writes it, and an `application/json` body names no `@context`.
    pub request: SubscriptionRequest,
}

impl ReducedSubscription {
    /// Reduce `subscription` to what `registration` matches, notifying
    /// `receiver`, under the identifier `minted`. The names the copy carries
    /// are spelled as the absolute IRIs the subscription stores them as, but
    /// for the `q` and the `csf`, which are spelled in the terms of `names`,
    /// the compaction of the `@context` the copy is written under.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Internal`] when a stored name has no
    /// written spelling, which the store never holds.
    pub async fn reduce(
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        receiver: &ReceiverEndpoint,
        minted: &Identifier,
        carriage: FilterCarriage,
        names: &NameCompaction,
    ) -> Result<ReducedSubscription, SubscriptionError> {
        let terms = WrittenSubscriptionTerms::try_from(subscription).map_err(SubscriptionInvariant::from)?;
        let q = copy_q(subscription, carriage, names).await.map_err(SubscriptionInvariant::from)?;
        let csf = copy_csf(subscription, names).await.map_err(SubscriptionInvariant::from)?;
        let selectors = reduced_selectors(terms.entities.as_deref(), &registration.coverage.information);
        let declared = declared_attributes(registration);
        let keeps_filters = carriage == FilterCarriage::Keep;

        // Clause 5.8.1.4: the notification member carries no `attributes`,
        // `pick` or `omit` whatever the carriage: the source sends its whole
        // part and this broker narrows the re-assembled entity. It keeps
        // `showChanges`: only the source holds the values its change replaced,
        // and the copy relays them (clause 5.8.6).
        let notification = NotificationRequest {
            format: Some(ResponseFormat::Normalized),
            endpoint: Some(receiver.notification_endpoint()),
            sys_attrs: subscription.notification.sys_attrs,
            show_changes: subscription.notification.show_changes,
            ..NotificationRequest::default()
        };

        Ok(ReducedSubscription {
            request: SubscriptionRequest {
                id: Some(minted.clone()),
                type_: Some(SubscriptionType),
                subscription_name: Clearable::from_option(subscription.subscription_name.clone()),
                description: Clearable::from_option(subscription.description.clone()),
                entities: Clearable::from_option(selectors.filter(|selectors| !selectors.is_empty())),
                watched_attributes: Clearable::from_option(copy_watched_attributes(subscription, terms.watched_attributes.as_deref(), declared.as_deref())),
                notification_trigger: Clearable::from_option(subscription.selection.notification_trigger.clone()),
                time_interval: Clearable::from_option(subscription.delivery.time_interval),
                q: Clearable::from_option(q),
                geo_q: Clearable::from_option(terms.geo_q.filter(|_| keeps_filters)),
                scope_q: Clearable::from_option(subscription.filters.scope_q.as_ref().filter(|_| keeps_filters).cloned()),
                csf: Clearable::from_option(csf),
                dataset_id: Clearable::from_option(copy_dataset_ids(subscription, registration)),
                notification: Some(notification),
                throttling: Clearable::from_option(subscription.delivery.throttling),
                expires_at: Clearable::from_option(subscription.lifecycle.standing.expires_at),
                ..SubscriptionRequest::default()
            },
        })
    }

    /// The same request as an update fragment: everything but the identifier
    /// and type (clause 5.8.2.4), and an NGSI-LD Null for each member the
    /// local update deleted that the copy at the source still holds
    /// (clauses 5.5.8 and 5.8.1.4, the copy being an update of the original
    /// reduced to what the registration matches).
    ///
    /// A member the copy never carries is not sent as a Null: the notification
    /// members always, and `q`, `geoQ` and `scopeQ` when `carriage` strips
    /// them. The members the registration narrows, `entities`,
    /// `watchedAttributes` and `datasetId`, are always sent, as the NGSI-LD
    /// Null when the copy names none: they follow the registration as well as
    /// the subscription, so a registration that stops naming entities,
    /// Attributes or `datasetId`s leaves the copy naming none though no update
    /// deleted the member. The selectors are sent only while the fragment
    /// sets one of them, since a source left with neither would refuse the
    /// update (clause 5.2.12). A periodic copy is so sent `watchedAttributes`
    /// as the NGSI-LD Null beside its `timeInterval`, which clause 5.2.1 makes
    /// the removal of the member rather than a value of it.
    #[must_use]
    pub fn as_fragment(&self, deleted: &DeletedMembers, carriage: FilterCarriage) -> SubscriptionRequest {
        let mut fragment = SubscriptionRequest {
            id: None,
            type_: None,
            ..self.request.clone()
        };
        fragment.subscription_name = deleting(fragment.subscription_name, DeletableMember::SubscriptionName, deleted);
        fragment.description = deleting(fragment.description, DeletableMember::Description, deleted);
        fragment.notification_trigger = deleting(fragment.notification_trigger, DeletableMember::NotificationTrigger, deleted);
        fragment.time_interval = deleting(fragment.time_interval, DeletableMember::TimeInterval, deleted);
        fragment.csf = deleting(fragment.csf, DeletableMember::Csf, deleted);
        fragment.dataset_id = narrowed(fragment.dataset_id);
        fragment.throttling = deleting(fragment.throttling, DeletableMember::Throttling, deleted);
        fragment.expires_at = deleting(fragment.expires_at, DeletableMember::ExpiresAt, deleted);
        match carriage {
            FilterCarriage::Keep => {
                fragment.q = deleting(fragment.q, DeletableMember::Q, deleted);
                fragment.geo_q = deleting(fragment.geo_q, DeletableMember::GeoQ, deleted);
                fragment.scope_q = deleting(fragment.scope_q, DeletableMember::ScopeQ, deleted);
            }
            FilterCarriage::Strip => {}
        }
        match (&fragment.entities, &fragment.watched_attributes) {
            (Clearable::Set(_), _) | (_, Clearable::Set(_)) => {
                fragment.entities = narrowed(fragment.entities);
                fragment.watched_attributes = narrowed(fragment.watched_attributes);
            }
            (Clearable::Absent | Clearable::NgsiLdNull, Clearable::Absent | Clearable::NgsiLdNull) => {}
        }
        fragment
    }
}

/// `member` as the fragment sends it: an absent member the update deleted
/// becomes the NGSI-LD Null, which deletes it at the source (clause 5.5.8),
/// anything else is as the reduction left it.
fn deleting<T>(member: Clearable<T>, kind: DeletableMember, deleted: &DeletedMembers) -> Clearable<T> {
    match member {
        Clearable::Absent if deleted.contains(kind) => Clearable::NgsiLdNull,
        other @ (Clearable::Absent | Clearable::NgsiLdNull | Clearable::Set(_)) => other,
    }
}

/// A member the registration narrows as the fragment sends it: absent becomes
/// the NGSI-LD Null, which deletes the value the copy at the source holds and
/// leaves a copy holding none as it is (clause 5.5.8), anything else is as the
/// reduction left it.
fn narrowed<T>(member: Clearable<T>) -> Clearable<T> {
    match member {
        Clearable::Absent => Clearable::NgsiLdNull,
        stated @ (Clearable::NgsiLdNull | Clearable::Set(_)) => stated,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        receiver::ReceiverEndpoint,
        reduced_copy::{FilterCarriage, ReducedSubscription},
    };
    use andromeda_federation::split_entities::SplitEntities;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_query::{
        filter::{Filter, expansion::expand_filter, json_key_attributes::JsonKeyAttributes},
        name_stage::{Expanded, Written},
    };
    use andromeda_subscription::{deleted_members::DeletedMembers, error::SubscriptionError};
    use futures::executor::block_on;
    use serde_json::{Value, json};
    use url::Url;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The compaction of `context`, which spells the filters of a copy written under it.
    fn names_in(context: &ContextSpec) -> NameCompaction {
        NameCompaction::new(block_on(offline_engine().compactor(context).processed_context()).expect("the context processes"))
    }

    /// The context whose body is `members`.
    fn context(members: &Value) -> ContextSpec {
        ContextSpec::from_body(JsonLiteral::admit(&members.to_string()).expect("an admissible literal"))
    }

    /// The copy of `subscription` at `registration`, spelled in `context`.
    fn reduced_in(
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        receiver: &ReceiverEndpoint,
        minted: &Identifier,
        carriage: FilterCarriage,
        context: &ContextSpec,
    ) -> Result<ReducedSubscription, SubscriptionError> {
        block_on(ReducedSubscription::reduce(
            subscription,
            registration,
            receiver,
            minted,
            carriage,
            &names_in(context),
        ))
    }

    /// The copy of `subscription` at `registration`, spelled in the core context.
    fn reduced(
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        receiver: &ReceiverEndpoint,
        minted: &Identifier,
        carriage: FilterCarriage,
    ) -> Result<ReducedSubscription, SubscriptionError> {
        reduced_in(subscription, registration, receiver, minted, carriage, &ContextSpec::none())
    }

    /// The request as the document the source reads.
    fn written(request: &SubscriptionRequest) -> Value {
        serde_json::to_value(request).expect("the request writes")
    }

    const EXPANDED_Q: &str = r#""https://example.org/speed">10"#;
    const VEHICLE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";
    const COLOUR: &str = "https://example.org/colour";

    /// The stored term tree of a projection member naming one attribute.
    fn stored_projection(name: &str) -> Value {
        json!([[{"name": name, "linked": []}]])
    }

    fn subscription() -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}, {"type": "https://example.org/Building"}],
            "watchedAttributes": [SPEED, COLOUR],
            "q": EXPANDED_Q,
            "notification": {
                "attributes": [SPEED, COLOUR],
                "pick": stored_projection(SPEED),
                "format": "keyValues",
                "endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}
            },
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:b",
            "type": "ContextSourceRegistration",
            "endpoint": "http://broker-b:1056",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn receiver() -> ReceiverEndpoint {
        ReceiverEndpoint::from_advertised(&Url::parse("http://broker-a:1056").expect("valid URL")).expect("receiver")
    }

    fn minted() -> Identifier {
        "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI")
    }

    #[test]
    fn the_copy_is_narrowed_to_the_registration_and_notifies_this_broker() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let copy = reduced(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep).expect("the subscription is reducible");
        let document = written(&copy.request);
        pretty_assertions::assert_eq!(document["id"], json!("urn:ngsi-ld:Subscription:forwarded-1"));
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE}]));
        pretty_assertions::assert_eq!(document["watchedAttributes"], json!([SPEED]));
        pretty_assertions::assert_eq!(document["q"], json!(EXPANDED_Q));
        pretty_assertions::assert_eq!(
            document["notification"]["endpoint"]["uri"],
            json!("http://broker-a:1056/ngsi-ld/andromeda/v1/notifications")
        );
        pretty_assertions::assert_eq!(document["notification"]["format"], json!("normalized"));
        assert!(document.get("@context").is_none());
    }

    /// The stored subscription of [`subscription`] whose `q` is `text`, read as the expanded filter it stores.
    fn subscription_with_q(text: &str) -> Subscription {
        let mut stored = subscription();
        stored.filters.q = Some(Filter::<Expanded>::try_from(text.to_owned()).expect("an expanded filter"));
        stored
    }

    /// The `q` the copy of `stored` carries, spelled in `context`.
    fn forwarded_q(stored: &Subscription, context: &ContextSpec) -> String {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let copy = reduced_in(stored, &registration, &receiver(), &minted(), FilterCarriage::Keep, context).expect("the subscription is reducible");
        written(&copy.request)["q"].as_str().expect("the copy carries a q").to_owned()
    }

    const ADDRESS_IN_CITY: &str = r#""https://example.org/address"["https://schema.example/city"]=="Berlin""#;

    /// Clause 5.5.7: the member `city` was expanded with a client's `@context` to the IRI the broker matches.
    /// The source expands the copy with the `@context` the copy is written under, so the `q` is spelled in
    /// that context: `city` where the context defines it, and the IRI where it does not.
    #[test]
    fn a_trailing_member_expanded_from_a_term_is_forwarded_as_the_term_of_the_forwarding_context() {
        let under_city = context(&json!({"city": "https://schema.example/city"}));

        let text = forwarded_q(&subscription_with_q(ADDRESS_IN_CITY), &under_city);

        pretty_assertions::assert_eq!(text, r#""https://example.org/address"[city]=="Berlin""#);
        let engine = offline_engine();
        let prepared = engine.prepare_term_context(&under_city);
        let reexpanded = block_on(expand_filter(
            text.parse::<Filter<Written>>().expect("the forwarded text is a q"),
            &JsonKeyAttributes::default(),
            &ContextualExpander::new(&engine, &prepared),
        ))
        .expect("the names expand");
        pretty_assertions::assert_eq!(reexpanded.to_string(), ADDRESS_IN_CITY);
    }

    /// Without a term for the member in the forwarding context, the IRI travels, and an IRI expands to itself.
    #[test]
    fn a_trailing_member_no_term_abbreviates_is_forwarded_as_its_iri() {
        let text = forwarded_q(&subscription_with_q(ADDRESS_IN_CITY), &ContextSpec::none());

        pretty_assertions::assert_eq!(text, ADDRESS_IN_CITY);
    }

    /// Clause 4.9: a `LanguageProperty` target language is read as written, and the broker stores the member the
    /// default vocabulary expanded it to, so the copy is spelled `[en]` again.
    #[test]
    fn a_trailing_member_expanded_from_a_language_tag_is_forwarded_as_the_tag() {
        let stored = subscription_with_q(r#""https://example.org/name"["https://uri.etsi.org/ngsi-ld/default-context/en"]=="Door""#);

        let text = forwarded_q(&stored, &ContextSpec::none());

        pretty_assertions::assert_eq!(text, r#""https://example.org/name"[en]=="Door""#);
    }

    /// Clause 4.9: a Context Source Filter names the Context Source Properties of a registration, spelled
    /// in the forwarding context like the attributes of a `q`.
    #[test]
    fn the_forwarded_csf_is_spelled_in_the_forwarding_context() {
        let mut stored = subscription();
        stored.filters.csf = Some(Filter::<Expanded>::try_from(r#""https://schema.example/tier"=="gold""#.to_owned()).expect("an expanded filter"));
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));

        let copy = reduced_in(
            &stored,
            &registration,
            &receiver(),
            &minted(),
            FilterCarriage::Keep,
            &context(&json!({"tier": "https://schema.example/tier"})),
        )
        .expect("the subscription is reducible");

        pretty_assertions::assert_eq!(written(&copy.request)["csf"], json!(r#"tier=="gold""#));
    }

    /// A subscription selecting the given entity selectors and nothing else.
    fn subscription_selecting(entities: &Value) -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": entities,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    /// Clause 5.12: a selector pattern matching the registered `id` selects it,
    /// and the copy names that `id` alone (clause 4.3.6.1).
    #[test]
    fn a_selector_by_pattern_reaches_the_source_as_the_registered_id_it_matches() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]}]));
        let selecting = subscription_selecting(&json!([
            {"type": VEHICLE, "idPattern": "urn:ngsi-ld:Vehicle:.*"},
            {"type": VEHICLE, "idPattern": "urn:ngsi-ld:Building:.*"}
        ]));
        let document = written(
            &reduced(&selecting, &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .expect("the subscription is reducible")
                .request,
        );
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]));
    }

    /// Clause 5.2.8: the registered `id` decides, so a wide pattern beside it
    /// does not make the block cover another identifier.
    #[test]
    fn a_selector_by_id_is_dropped_for_a_block_registering_another_id_beside_a_wide_pattern() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7", "idPattern": ".*"}]}]));
        let selecting = subscription_selecting(&json!([
            {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}
        ]));
        let document = written(
            &reduced(&selecting, &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .expect("the subscription is reducible")
                .request,
        );
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]));
    }

    #[test]
    fn split_entities_strip_the_filters() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let copy = reduced(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Strip).expect("the subscription is reducible");
        let document = written(&copy.request);
        assert!(document.get("q").is_none());
        // A registration declaring no attribute covers every watched one.
        pretty_assertions::assert_eq!(document["watchedAttributes"], json!([SPEED, COLOUR]));
    }

    /// A subscription asking for `showChanges` as `show_changes` says.
    fn subscription_showing_changes(show_changes: Option<bool>) -> Subscription {
        let mut notification = json!({"endpoint": {"uri": "http://subscriber.example.org/notify"}});
        if let (Some(target), Some(show_changes)) = (notification.as_object_mut(), show_changes) {
            target.insert("showChanges".to_owned(), json!(show_changes));
        }
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}],
            "notification": notification,
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    /// Clause 5.8.1.4 removes only `attributes`, `pick` and `omit` from the
    /// copy's notification member, so `showChanges` travels with it, on the
    /// copy and on every update fragment, whatever the carriage; a local
    /// subscription that stops asking for it sends a member without it.
    #[test]
    fn the_copy_keeps_show_changes() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let copy =
                reduced(&subscription_showing_changes(Some(true)), &registration, &receiver(), &minted(), carriage).expect("the subscription is reducible");
            pretty_assertions::assert_eq!(written(&copy.request)["notification"]["showChanges"], json!(true), "{carriage:?}");
            let fragment = written(&copy.as_fragment(&DeletedMembers::default(), carriage));
            pretty_assertions::assert_eq!(fragment["notification"]["showChanges"], json!(true), "{carriage:?}");
        }
        for show_changes in [None, Some(false)] {
            let copy = reduced(
                &subscription_showing_changes(show_changes),
                &registration,
                &receiver(),
                &minted(),
                FilterCarriage::Keep,
            )
            .expect("the subscription is reducible");
            let document = written(&copy.request);
            assert_ne!(document["notification"].get("showChanges"), Some(&json!(true)), "{show_changes:?}: {document}");
        }
    }

    /// Clause 5.8.1.4: `q`, `geoQ` and `scopeQ` travel only when entities are
    /// complete at every source, while the notification's `attributes`,
    /// `pick` and `omit` never travel, whatever the carriage and whichever
    /// of the three the subscription set.
    #[test]
    fn the_reduce_matrix_keeps_filters_by_carriage_and_never_the_projection_members() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let projections = [
            json!({"attributes": [SPEED, COLOUR]}),
            json!({"pick": stored_projection(SPEED)}),
            json!({"omit": stored_projection(COLOUR)}),
            json!({"attributes": [SPEED], "pick": stored_projection(SPEED), "omit": stored_projection(COLOUR)}),
            json!({}),
        ];
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            for projection in &projections {
                let mut notification = json!({"endpoint": {"uri": "http://subscriber.example.org/notify"}});
                if let (Some(target), Some(members)) = (notification.as_object_mut(), projection.as_object()) {
                    target.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
                }
                let subscription: Subscription = serde_json::from_value(json!({
                    "id": "urn:ngsi-ld:Subscription:local",
                    "type": "Subscription",
                    "entities": [{"type": VEHICLE}],
                    "q": EXPANDED_Q,
                    "scopeQ": "/Madrid",
                    "notification": notification,
                    "status": "active",
                    "createdAt": "2024-01-01T00:00:00Z",
                    "modifiedAt": "2024-01-01T00:00:00Z"
                }))
                .expect("valid subscription");

                let document = written(
                    &reduced(&subscription, &registration, &receiver(), &minted(), carriage)
                        .expect("the subscription is reducible")
                        .request,
                );

                for member in ["attributes", "pick", "omit"] {
                    assert!(
                        document["notification"].get(member).is_none(),
                        "{member} must never travel under {carriage:?} with {projection}: {document}"
                    );
                }
                let filters_travel = carriage == FilterCarriage::Keep;
                pretty_assertions::assert_eq!(document.get("q").is_some(), filters_travel, "q under {carriage:?}: {document}");
                pretty_assertions::assert_eq!(document.get("scopeQ").is_some(), filters_travel, "scopeQ under {carriage:?}: {document}");
            }
        }
    }

    /// The `csf` of the subscriptions below, as the store holds it, its
    /// property term expanded.
    const GOLD_TIER: &str = r#""https://uri.etsi.org/ngsi-ld/default-context/tier"=="gold""#;
    /// [`GOLD_TIER`] as the copy carries it: the core context a copy without a `jsonldContext` is written under abbreviates `tier`.
    const GOLD_TIER_FORWARDED: &str = r#"tier=="gold""#;

    /// A subscription of vehicles with the context source filter `csf`, when
    /// there is one.
    fn subscription_filtering_sources(csf: Option<&str>) -> Subscription {
        let mut document = json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}],
            "q": EXPANDED_Q,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(csf)) = (document.as_object_mut(), csf) {
            target.insert("csf".to_owned(), json!(csf));
        }
        serde_json::from_value(document).expect("valid subscription")
    }

    /// Clause 5.8.1.4 reduces the copy by `q`, `geoQ`, `scopeQ` and the
    /// notification's projection alone, and `csf` decides no entity (Table
    /// 5.2.12-1), so it travels with the copy and with every update fragment
    /// spelled in the `@context` the copy is written under, under split entities and without.
    #[test]
    fn the_copy_keeps_the_context_source_filter_whatever_the_carriage() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let local = subscription_filtering_sources(Some(GOLD_TIER));
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let copy = reduced(&local, &registration, &receiver(), &minted(), carriage).expect("the subscription is reducible");
            pretty_assertions::assert_eq!(written(&copy.request)["csf"], json!(GOLD_TIER_FORWARDED), "{carriage:?}");
            let fragment = written(&copy.as_fragment(&DeletedMembers::default(), carriage));
            pretty_assertions::assert_eq!(fragment["csf"], json!(GOLD_TIER_FORWARDED), "{carriage:?}");
        }
        let unfiltered = written(
            &reduced(
                &subscription_filtering_sources(None),
                &registration,
                &receiver(),
                &minted(),
                FilterCarriage::Keep,
            )
            .expect("the subscription is reducible")
            .request,
        );
        assert!(unfiltered.get("csf").is_none(), "{unfiltered}");
    }

    /// Clauses 5.5.8 and 5.8.2.4: an update deleting `csf` reaches the copy as
    /// the NGSI-LD Null whatever the carriage, since the copy always carries
    /// it, and one leaving it out sends none.
    #[test]
    fn a_deleted_context_source_filter_is_forwarded_as_the_ngsi_ld_null() {
        let local = subscription_filtering_sources(None);
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let deleted = fragment_after(&local, &json!({"csf": "urn:ngsi-ld:null"}), carriage);
            pretty_assertions::assert_eq!(deleted["csf"], json!("urn:ngsi-ld:null"), "{carriage:?}");
            let untouched = fragment_after(&local, &json!({"throttling": 5}), carriage);
            assert!(untouched.get("csf").is_none(), "{carriage:?}: {untouched}");
        }
    }

    /// Clause 5.8.1.4: an update is reduced to what the registration matches
    /// as the copy is, so the fragment carries the narrowed selectors.
    #[test]
    fn the_update_fragment_carries_the_narrowed_selectors() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]}]));
        let fragment = written(
            &reduced(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .expect("the subscription is reducible")
                .as_fragment(&DeletedMembers::default(), FilterCarriage::Keep),
        );
        pretty_assertions::assert_eq!(fragment["entities"], json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]));
    }

    const DATASET_A: &str = "urn:ngsi-ld:Dataset:a";
    const DATASET_B: &str = "urn:ngsi-ld:Dataset:b";
    const DEFAULT_INSTANCE: &str = "@none";

    /// A subscription of vehicles naming the `datasetId`s `named`, when it
    /// names any.
    fn subscription_naming(named: Option<&[&str]>) -> Subscription {
        let mut document = json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}],
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(named)) = (document.as_object_mut(), named) {
            target.insert("datasetId".to_owned(), json!(named));
        }
        serde_json::from_value(document).expect("valid subscription")
    }

    /// A registration of vehicles listing the `datasetId`s `listed`, when it
    /// lists any.
    fn registration_listing(listed: Option<&[&str]>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:b",
            "type": "ContextSourceRegistration",
            "endpoint": "http://broker-b:1056",
            "information": [{"entities": [{"type": VEHICLE}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(listed)) = (document.as_object_mut(), listed) {
            target.insert("datasetId".to_owned(), json!(listed));
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// Clause 4.3.6.1 and Table 5.2.9-1: the copy names the subscription's
    /// `datasetId`s its registration lists, in the subscription's order, every
    /// listed one when the subscription names none, and the subscription's own
    /// when the registration lists none.
    #[test]
    fn the_copy_names_only_the_dataset_ids_its_registration_lists() {
        for (named, listed, carried) in [
            (
                Some(&[DATASET_B, DATASET_A][..]),
                Some(&[DATASET_A, DEFAULT_INSTANCE, DATASET_B][..]),
                Some(json!([DATASET_B, DATASET_A])),
            ),
            (Some(&[DATASET_A, DATASET_B][..]), Some(&[DATASET_A][..]), Some(json!([DATASET_A]))),
            (None, Some(&[DEFAULT_INSTANCE, DATASET_A][..]), Some(json!([DEFAULT_INSTANCE, DATASET_A]))),
            (Some(&[DATASET_A, DATASET_B][..]), None, Some(json!([DATASET_A, DATASET_B]))),
            (None, None, None),
        ] {
            for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
                let copy = reduced(&subscription_naming(named), &registration_listing(listed), &receiver(), &minted(), carriage)
                    .expect("the subscription is reducible");
                pretty_assertions::assert_eq!(
                    written(&copy.request).get("datasetId"),
                    carried.as_ref(),
                    "{named:?} against {listed:?} under {carriage:?}"
                );
            }
        }
    }

    /// The copy's `datasetId` follows the registration as well as the
    /// subscription, so every update fragment states it: the values the copy
    /// names, or the NGSI-LD Null when it names none, which a registration
    /// that stops listing `datasetId`s leaves it naming (clause 5.5.8).
    #[test]
    fn every_update_fragment_states_the_copy_dataset_ids() {
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let narrowed = reduced(
                &subscription_naming(None),
                &registration_listing(Some(&[DATASET_A])),
                &receiver(),
                &minted(),
                carriage,
            )
            .expect("the subscription is reducible");
            pretty_assertions::assert_eq!(
                written(&narrowed.as_fragment(&DeletedMembers::default(), carriage))["datasetId"],
                json!([DATASET_A]),
                "{carriage:?}"
            );
            let unnarrowed =
                reduced(&subscription_naming(None), &registration_listing(None), &receiver(), &minted(), carriage).expect("the subscription is reducible");
            let fragment = written(&unnarrowed.as_fragment(&DeletedMembers::default(), carriage));
            pretty_assertions::assert_eq!(fragment["datasetId"], json!(["urn:ngsi-ld:null"]), "{carriage:?}");
            assert!(
                written(&unnarrowed.request).get("datasetId").is_none(),
                "a create names no NGSI-LD Null: {carriage:?}"
            );
        }
    }

    #[test]
    fn the_update_fragment_drops_the_identifier_and_type() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let fragment = written(
            &reduced(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .expect("the subscription is reducible")
                .as_fragment(&DeletedMembers::default(), FilterCarriage::Keep),
        );
        assert!(fragment.get("id").is_none());
        assert!(fragment.get("type").is_none());
        assert!(fragment.get("entities").is_some());
    }

    /// The fragment a local update of `local` produces for a registration of
    /// `information`, with the members `update` deleted.
    fn fragment_under(information: &Value, local: &Subscription, update: &Value, carriage: FilterCarriage) -> Value {
        let registration = registration(information);
        let deleted = DeletedMembers::of(&serde_json::from_value(update.clone()).expect("a readable fragment"));
        written(
            &reduced(local, &registration, &receiver(), &minted(), carriage)
                .expect("the subscription is reducible")
                .as_fragment(&deleted, carriage),
        )
    }

    /// The fragment a local update of `local` produces for a registration of
    /// vehicles, with the members `update` deleted.
    fn fragment_after(local: &Subscription, update: &Value, carriage: FilterCarriage) -> Value {
        fragment_under(&json!([{"entities": [{"type": VEHICLE}]}]), local, update, carriage)
    }

    /// Registration information constraining neither the entities nor the
    /// attributes of a subscription naming none: one block registers no
    /// entity, the other no attribute.
    fn unconstraining_information() -> Value {
        json!([{"entities": [{"type": VEHICLE}]}, {"propertyNames": [SPEED]}])
    }

    /// Clauses 5.5.8 and 5.8.1.4: a member the local update deleted is sent to
    /// the source as an NGSI-LD Null, and one the update set or left alone is not.
    #[test]
    fn a_locally_deleted_member_is_forwarded_as_the_ngsi_ld_null() {
        let mut local = subscription();
        local.lifecycle.standing.expires_at = None;
        local.description = None;
        let fragment = fragment_after(
            &local,
            &json!({"expiresAt": "urn:ngsi-ld:null", "description": "urn:ngsi-ld:null", "throttling": 5}),
            FilterCarriage::Keep,
        );
        pretty_assertions::assert_eq!(fragment["expiresAt"], json!("urn:ngsi-ld:null"));
        pretty_assertions::assert_eq!(fragment["description"], json!("urn:ngsi-ld:null"));
        assert!(fragment.get("subscriptionName").is_none());
        assert!(fragment.get("throttling").is_none());
        assert!(fragment.get("notification").is_some());
    }

    /// Clause 5.8.1.4: a copy under split entities never carries `q`, so its
    /// deletion is not sent; the notification members are never sent either.
    #[test]
    fn a_deleted_member_the_copy_never_carries_is_not_sent() {
        let mut local = subscription();
        local.filters.q = None;
        let update = json!({"q": "urn:ngsi-ld:null", "notification": {"attributes": [SPEED]}});
        let stripped = fragment_after(&local, &update, FilterCarriage::Strip);
        assert!(stripped.get("q").is_none());
        pretty_assertions::assert_eq!(stripped["notification"].get("attributes"), None);
        let kept = fragment_after(&local, &json!({"q": "urn:ngsi-ld:null"}), FilterCarriage::Keep);
        pretty_assertions::assert_eq!(kept["q"], json!("urn:ngsi-ld:null"));
    }

    /// Clause 5.2.12: deleting `entities` is forwarded while the copy still
    /// watches attributes, and a selector is never deleted when the fragment
    /// would leave the source with neither.
    #[test]
    fn a_selector_is_deleted_only_while_the_other_remains() {
        let mut local = subscription();
        local.selection.entities = None;
        let deleting_entities = json!({"entities": "urn:ngsi-ld:null"});
        let with_watched = fragment_under(&unconstraining_information(), &local, &deleting_entities, FilterCarriage::Keep);
        pretty_assertions::assert_eq!(with_watched["entities"], json!(["urn:ngsi-ld:null"]));
        pretty_assertions::assert_eq!(with_watched["watchedAttributes"], json!([SPEED, COLOUR]));

        local.selection.watched_attributes = None;
        let neither = fragment_under(&unconstraining_information(), &local, &deleting_entities, FilterCarriage::Keep);
        assert!(neither.get("entities").is_none());
    }

    /// Clause 4.3.6.1: a subscription that deleted its selectors still reaches
    /// a source registered for one type with that type alone.
    #[test]
    fn a_deleted_selection_reaches_the_source_as_the_registered_entities() {
        let mut local = subscription();
        local.selection.entities = None;
        let fragment = fragment_after(&local, &json!({"entities": "urn:ngsi-ld:null"}), FilterCarriage::Keep);
        pretty_assertions::assert_eq!(fragment["entities"], json!([{"type": VEHICLE}]));
    }

    /// Clauses 5.5.8 and 5.8.1.4: the copy watches what the registration
    /// names while the subscription names nothing, so once the registration
    /// names no Attribute the update deletes `watchedAttributes` at the source
    /// and keeps the selectors, whatever the carriage.
    #[test]
    fn a_registration_that_stops_naming_attributes_deletes_the_watched_attributes_it_narrowed() {
        let local = subscription_selecting(&json!([{"type": VEHICLE}]));
        let nothing_deleted = json!({"throttling": 5});
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let narrowing = fragment_under(
                &json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]),
                &local,
                &nothing_deleted,
                carriage,
            );
            pretty_assertions::assert_eq!(narrowing["watchedAttributes"], json!([SPEED]), "{carriage:?}");
            let widened = fragment_after(&local, &nothing_deleted, carriage);
            pretty_assertions::assert_eq!(widened["watchedAttributes"], json!(["urn:ngsi-ld:null"]), "{carriage:?}");
            pretty_assertions::assert_eq!(widened["entities"], json!([{"type": VEHICLE}]), "{carriage:?}");
        }
    }

    /// Clauses 5.5.8 and 5.8.1.4: the copy of a subscription naming no
    /// entities selects what the registration registers, so once the
    /// registration registers none the update deletes `entities` at the
    /// source and keeps the watched Attributes, whatever the carriage.
    #[test]
    fn a_registration_that_stops_naming_entities_deletes_the_selectors_it_narrowed() {
        let mut local = subscription();
        local.selection.entities = None;
        let nothing_deleted = json!({"throttling": 5});
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let narrowing = fragment_under(
                &json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]),
                &local,
                &nothing_deleted,
                carriage,
            );
            pretty_assertions::assert_eq!(narrowing["entities"], json!([{"type": VEHICLE}]), "{carriage:?}");
            let widened = fragment_under(&json!([{"propertyNames": [SPEED]}]), &local, &nothing_deleted, carriage);
            pretty_assertions::assert_eq!(widened["entities"], json!(["urn:ngsi-ld:null"]), "{carriage:?}");
            pretty_assertions::assert_eq!(widened["watchedAttributes"], json!([SPEED]), "{carriage:?}");
        }
    }

    /// Table 5.2.12-1 gives `watchedAttributes` no place beside
    /// `timeInterval`, so the copy of a periodic subscription at a
    /// registration naming Attributes watches nothing, and every update states
    /// the member as the NGSI-LD Null, the removal of a member rather than a
    /// value of it (clause 5.2.1), which deletes what a copy that watched
    /// before holds.
    #[test]
    fn a_periodic_copy_watches_no_attribute_beside_its_time_interval() {
        let mut periodic = subscription_selecting(&json!([{"type": VEHICLE}]));
        periodic.delivery.time_interval = Some(60.0);
        let naming = json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]);
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            let copy = written(
                &reduced(&periodic, &registration(&naming), &receiver(), &minted(), carriage)
                    .expect("the subscription is reducible")
                    .request,
            );
            pretty_assertions::assert_eq!(copy["timeInterval"], json!(60), "{carriage:?}");
            assert!(copy.get("watchedAttributes").is_none(), "{carriage:?}: {copy}");
            let fragment = fragment_under(&naming, &periodic, &json!({"subscriptionName": "fleet"}), carriage);
            pretty_assertions::assert_eq!(fragment["timeInterval"], json!(60), "{carriage:?}");
            pretty_assertions::assert_eq!(fragment["watchedAttributes"], json!(["urn:ngsi-ld:null"]), "{carriage:?}");
            pretty_assertions::assert_eq!(fragment["entities"], json!([{"type": VEHICLE}]), "{carriage:?}");
        }
    }

    #[test]
    fn a_registration_that_covers_none_of_the_selectors_yields_no_entities_member() {
        let registration = registration(&json!([{"entities": [{"type": "https://example.org/Sensor"}]}]));
        let document = written(
            &reduced(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .expect("the subscription is reducible")
                .request,
        );
        assert!(document.get("entities").is_none());
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Split), FilterCarriage::Strip);
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Complete), FilterCarriage::Keep);
    }
}
