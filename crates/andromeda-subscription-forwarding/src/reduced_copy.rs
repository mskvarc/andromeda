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
//! source.

use crate::receiver::ReceiverEndpoint;
use andromeda_federation::split_entities::SplitEntities;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    notification_request::NotificationRequest,
    registration::ContextSourceRegistration,
    registration_matching::selector_reduction::reduced_selectors,
    subscription::{Subscription, SubscriptionType},
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_subscription::deleted_members::{DeletableMember, DeletedMembers};

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

/// The reduced subscription placed at a context source.
#[derive(Debug, Clone)]
pub struct ReducedSubscription {
    /// The create body placed at the source, with expanded IRIs; the
    /// transport writes it, and an `application/json` body names no `@context`.
    pub request: SubscriptionRequest,
}

impl ReducedSubscription {
    /// Reduce `subscription` to what `registration` matches, notifying
    /// `receiver`, under the identifier `minted`.
    #[must_use]
    pub fn reduce(
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        receiver: &ReceiverEndpoint,
        minted: &Identifier,
        carriage: FilterCarriage,
    ) -> ReducedSubscription {
        let selectors = reduced_selectors(subscription.selection.entities.as_deref(), &registration.coverage.information);
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

        ReducedSubscription {
            request: SubscriptionRequest {
                id: Some(minted.clone()),
                type_: Some(SubscriptionType),
                subscription_name: Clearable::from_option(subscription.subscription_name.clone()),
                description: Clearable::from_option(subscription.description.clone()),
                entities: Clearable::from_option(selectors.filter(|selectors| !selectors.is_empty())),
                watched_attributes: Clearable::from_option(narrow_names(subscription.selection.watched_attributes.as_deref(), declared.as_deref())),
                notification_trigger: Clearable::from_option(subscription.selection.notification_trigger.clone()),
                time_interval: Clearable::from_option(subscription.delivery.time_interval),
                q: Clearable::from_option(subscription.filters.q.as_ref().filter(|_| keeps_filters).cloned()),
                geo_q: Clearable::from_option(subscription.filters.geo_q.as_ref().filter(|_| keeps_filters).cloned()),
                scope_q: Clearable::from_option(subscription.filters.scope_q.as_ref().filter(|_| keeps_filters).cloned()),
                dataset_id: Clearable::from_option(subscription.filters.dataset_id.clone()),
                notification: Some(notification),
                throttling: Clearable::from_option(subscription.delivery.throttling),
                expires_at: Clearable::from_option(subscription.lifecycle.standing.expires_at),
                ..SubscriptionRequest::default()
            },
        }
    }

    /// The same request as an update fragment: everything but the identifier
    /// and type (clause 5.8.2.4), and an NGSI-LD Null for each member the
    /// local update deleted that the copy at the source still holds
    /// (clauses 5.5.8 and 5.8.1.4, the copy being an update of the original
    /// reduced to what the registration matches).
    ///
    /// A member the copy never carries is not sent as a Null: the notification
    /// members always, and `q`, `geoQ` and `scopeQ` when `carriage` strips
    /// them. The selectors are deleted only while the fragment still sets the
    /// other one, since a source left with neither would refuse the update
    /// (clause 5.2.12).
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
        fragment.dataset_id = deleting(fragment.dataset_id, DeletableMember::DatasetId, deleted);
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
                fragment.entities = deleting(fragment.entities, DeletableMember::Entities, deleted);
                fragment.watched_attributes = deleting(fragment.watched_attributes, DeletableMember::WatchedAttributes, deleted);
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

#[cfg(test)]
mod tests {
    use crate::{
        receiver::ReceiverEndpoint,
        reduced_copy::{FilterCarriage, ReducedSubscription},
    };
    use andromeda_federation::split_entities::SplitEntities;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription, subscription_request::SubscriptionRequest};
    use andromeda_subscription::deleted_members::DeletedMembers;
    use serde_json::{Value, json};
    use url::Url;

    /// The request as the document the source reads.
    fn written(request: &SubscriptionRequest) -> Value {
        serde_json::to_value(request).expect("the request writes")
    }

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
            "q": "speed>10",
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
        let copy = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep);
        let document = written(&copy.request);
        pretty_assertions::assert_eq!(document["id"], json!("urn:ngsi-ld:Subscription:forwarded-1"));
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE}]));
        pretty_assertions::assert_eq!(document["watchedAttributes"], json!([SPEED]));
        pretty_assertions::assert_eq!(document["q"], json!("speed>10"));
        pretty_assertions::assert_eq!(
            document["notification"]["endpoint"]["uri"],
            json!("http://broker-a:1056/ngsi-ld/andromeda/v1/notifications")
        );
        pretty_assertions::assert_eq!(document["notification"]["format"], json!("normalized"));
        assert!(document.get("@context").is_none());
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
        let document = written(&ReducedSubscription::reduce(&selecting, &registration, &receiver(), &minted(), FilterCarriage::Keep).request);
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
        let document = written(&ReducedSubscription::reduce(&selecting, &registration, &receiver(), &minted(), FilterCarriage::Keep).request);
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]));
    }

    #[test]
    fn split_entities_strip_the_filters() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let copy = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Strip);
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
            let copy = ReducedSubscription::reduce(&subscription_showing_changes(Some(true)), &registration, &receiver(), &minted(), carriage);
            pretty_assertions::assert_eq!(written(&copy.request)["notification"]["showChanges"], json!(true), "{carriage:?}");
            let fragment = written(&copy.as_fragment(&DeletedMembers::default(), carriage));
            pretty_assertions::assert_eq!(fragment["notification"]["showChanges"], json!(true), "{carriage:?}");
        }
        for show_changes in [None, Some(false)] {
            let copy = ReducedSubscription::reduce(
                &subscription_showing_changes(show_changes),
                &registration,
                &receiver(),
                &minted(),
                FilterCarriage::Keep,
            );
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
                    "q": "speed>10",
                    "scopeQ": "/Madrid",
                    "notification": notification,
                    "status": "active",
                    "createdAt": "2024-01-01T00:00:00Z",
                    "modifiedAt": "2024-01-01T00:00:00Z"
                }))
                .expect("valid subscription");

                let document = written(&ReducedSubscription::reduce(&subscription, &registration, &receiver(), &minted(), carriage).request);

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

    /// Clause 5.8.1.4: an update is reduced to what the registration matches
    /// as the copy is, so the fragment carries the narrowed selectors.
    #[test]
    fn the_update_fragment_carries_the_narrowed_selectors() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]}]));
        let fragment = written(
            &ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep)
                .as_fragment(&DeletedMembers::default(), FilterCarriage::Keep),
        );
        pretty_assertions::assert_eq!(fragment["entities"], json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:7"}]));
    }

    #[test]
    fn the_update_fragment_drops_the_identifier_and_type() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let fragment = written(
            &ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep)
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
        written(&ReducedSubscription::reduce(local, &registration, &receiver(), &minted(), carriage).as_fragment(&deleted, carriage))
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

    #[test]
    fn a_registration_that_covers_none_of_the_selectors_yields_no_entities_member() {
        let registration = registration(&json!([{"entities": [{"type": "https://example.org/Sensor"}]}]));
        let document = written(&ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep).request);
        assert!(document.get("entities").is_none());
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Split), FilterCarriage::Strip);
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Complete), FilterCarriage::Keep);
    }
}
