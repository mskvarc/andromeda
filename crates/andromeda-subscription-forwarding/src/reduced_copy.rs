//! Reducing a subscription to what one registration matches (clause 5.8.1.4).
//!
//! The copy placed at a context source watches only the entities and attributes
//! the registration declared and notifies this broker rather than the
//! subscriber. The `attributes`, `pick` and `omit` of the notification member
//! never travel with it: this broker narrows the entities it re-assembles
//! before delivering them (clause 5.8.6), and a source that had already
//! narrowed its part would leave nothing to re-assemble from. The filters
//! `q`, `geoQ` and `scopeQ` go only when entities may be split, because only
//! then can they not be decided at one source.

use crate::receiver::ReceiverEndpoint;
use andromeda_federation::config::SplitEntities;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    registration::{ContextSourceRegistration, RegistrationInfo},
    subscription::{EntitySelector, Subscription},
};
use serde_json::{Map, Value, json};

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

/// Whether a registration block's entity selectors cover a subscription selector
/// (clause 5.12): the types agree when both constrain them, and an identifier
/// constraint on either side admits the other.
fn block_covers_selector(block: &RegistrationInfo, selector: &EntitySelector) -> bool {
    let Some(entities) = block.entities.as_deref() else {
        return true;
    };
    entities.iter().any(|registered| {
        let type_matches = selector.type_.matches(registered.type_.names());
        // Clause 5.12: an identifier constraint on one side is matched against
        // the other side's identifier or pattern; two patterns, or no
        // constraint on either side, are taken as matching.
        let id_matches = match (&registered.id, &selector.id, &registered.id_pattern, &selector.id_pattern) {
            (Some(registered_id), Some(selected_id), _, _) => registered_id == selected_id,
            (Some(registered_id), None, _, Some(pattern)) => pattern.is_match(registered_id.as_str()),
            (None, Some(selected_id), Some(pattern), _) => pattern.is_match(selected_id.as_str()),
            (None, None, Some(_), Some(_) | None) | (None, _, None, _) | (Some(_), None, _, None) => true,
        };
        type_matches && id_matches
    })
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
        (None, Some(declared)) => Some(declared.iter().map(|name| (*name).clone()).collect()), // clone: the copy owns its names
        (Some(names), None) => Some(names.to_vec()),                                           // clone: same
        (Some(names), Some(declared)) => Some(names.iter().filter(|name| declared.contains(name)).cloned().collect()), // clone: same
    }
}

/// The reduced subscription placed at a context source.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducedSubscription {
    /// The JSON document, with expanded IRIs and without an inline `@context`.
    pub document: Value,
}

impl ReducedSubscription {
    /// Reduce `subscription` to what `registration` matches, notifying
    /// `receiver`, under the identifier `minted`.
    #[must_use]
    pub fn reduce(
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        receiver: &ReceiverEndpoint,
        minted: &Uri,
        carriage: FilterCarriage,
    ) -> ReducedSubscription {
        let mut document = Map::new();
        document.insert("id".to_string(), json!(minted));
        document.insert("type".to_string(), json!("Subscription"));
        if let Some(name) = &subscription.subscription_name {
            document.insert("subscriptionName".to_string(), json!(name));
        }
        if let Some(description) = &subscription.description {
            document.insert("description".to_string(), json!(description));
        }

        let selectors: Vec<&EntitySelector> = subscription
            .selection
            .entities
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|selector| registration.coverage.information.iter().any(|block| block_covers_selector(block, selector)))
            .collect();
        if !selectors.is_empty() {
            document.insert("entities".to_string(), json!(selectors));
        }

        let declared = declared_attributes(registration);
        if let Some(watched) = narrow_names(subscription.selection.watched_attributes.as_deref(), declared.as_deref()) {
            document.insert("watchedAttributes".to_string(), json!(watched));
        }
        if let Some(triggers) = &subscription.selection.notification_trigger {
            document.insert("notificationTrigger".to_string(), json!(triggers));
        }

        if carriage == FilterCarriage::Keep {
            if let Some(q) = &subscription.filters.q {
                document.insert("q".to_string(), json!(q.to_string()));
            }
            if let Some(geo_q) = &subscription.filters.geo_q {
                document.insert("geoQ".to_string(), geo_q.clone()); // clone: the copy owns its filter
            }
            if let Some(scope_q) = &subscription.filters.scope_q {
                document.insert("scopeQ".to_string(), json!(scope_q.to_string()));
            }
        }
        if let Some(dataset_id) = &subscription.filters.dataset_id {
            document.insert("datasetId".to_string(), json!(dataset_id));
        }
        if let Some(time_interval) = subscription.delivery.time_interval {
            document.insert("timeInterval".to_string(), json!(time_interval));
        }
        if let Some(throttling) = subscription.delivery.throttling {
            document.insert("throttling".to_string(), json!(throttling));
        }
        if let Some(expires_at) = subscription.lifecycle.expires_at {
            document.insert("expiresAt".to_string(), json!(expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)));
        }

        // Clause 5.8.1.4: the notification member carries no `attributes`,
        // `pick` or `omit` whatever the carriage -- the source sends its whole
        // part and this broker narrows the re-assembled entity.
        let mut notification = Map::new();
        notification.insert("format".to_string(), json!("normalized"));
        notification.insert("endpoint".to_string(), json!({"uri": receiver.url(), "accept": "application/json"}));
        if let Some(sys_attrs) = subscription.notification.sys_attrs {
            notification.insert("sysAttrs".to_string(), json!(sys_attrs));
        }
        document.insert("notification".to_string(), Value::Object(notification));

        ReducedSubscription {
            document: Value::Object(document),
        }
    }

    /// The same document as an update fragment: everything but the identifier
    /// and type (clause 5.8.2.4).
    #[must_use]
    pub fn as_fragment(&self) -> Value {
        let mut fragment = self.document.clone(); // clone: the fragment is a second document
        if let Some(object) = fragment.as_object_mut() {
            object.remove("id");
            object.remove("type");
        }
        fragment
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        receiver::ReceiverEndpoint,
        reduced_copy::{FilterCarriage, ReducedSubscription},
    };
    use andromeda_federation::config::SplitEntities;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
    use serde_json::json;
    use url::Url;

    const VEHICLE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";
    const COLOUR: &str = "https://example.org/colour";

    fn subscription() -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}, {"type": "https://example.org/Building"}],
            "watchedAttributes": [SPEED, COLOUR],
            "q": "speed>10",
            "notification": {
                "attributes": [SPEED, COLOUR],
                "pick": [SPEED],
                "format": "keyValues",
                "endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}
            },
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    fn registration(information: &serde_json::Value) -> ContextSourceRegistration {
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

    fn minted() -> Uri {
        "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI")
    }

    #[test]
    fn the_copy_is_narrowed_to_the_registration_and_notifies_this_broker() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let copy = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep);
        let document = copy.document;
        pretty_assertions::assert_eq!(document["id"], json!("urn:ngsi-ld:Subscription:forwarded-1"));
        pretty_assertions::assert_eq!(document["entities"], json!([{"type": VEHICLE}]));
        pretty_assertions::assert_eq!(document["watchedAttributes"], json!([SPEED]));
        pretty_assertions::assert_eq!(document["q"], json!("speed>10"));
        pretty_assertions::assert_eq!(
            document["notification"]["endpoint"]["uri"],
            json!("http://broker-a:1056/ngsi-ld/v1/notifications")
        );
        pretty_assertions::assert_eq!(document["notification"]["format"], json!("normalized"));
        assert!(document.get("@context").is_none());
    }

    #[test]
    fn split_entities_strip_the_filters() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let copy = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Strip);
        let document = copy.document;
        assert!(document.get("q").is_none());
        // A registration declaring no attribute covers every watched one.
        pretty_assertions::assert_eq!(document["watchedAttributes"], json!([SPEED, COLOUR]));
    }

    /// Clause 5.8.1.4: `q`, `geoQ` and `scopeQ` travel only when entities are
    /// complete at every source, while the notification's `attributes`,
    /// `pick` and `omit` never travel -- whatever the carriage and whichever
    /// of the three the subscription set.
    #[test]
    fn the_reduce_matrix_keeps_filters_by_carriage_and_never_the_projection_members() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]));
        let projections = [
            json!({"attributes": [SPEED, COLOUR]}),
            json!({"pick": [SPEED]}),
            json!({"omit": [COLOUR]}),
            json!({"attributes": [SPEED], "pick": [SPEED], "omit": [COLOUR]}),
            json!({}),
        ];
        for carriage in [FilterCarriage::Keep, FilterCarriage::Strip] {
            for projection in &projections {
                let mut notification = json!({"endpoint": {"uri": "http://subscriber.example.org/notify"}});
                if let (Some(target), Some(members)) = (notification.as_object_mut(), projection.as_object()) {
                    target.extend(members.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: each case builds its own document
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

                let document = ReducedSubscription::reduce(&subscription, &registration, &receiver(), &minted(), carriage).document;

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

    #[test]
    fn the_update_fragment_drops_the_identifier_and_type() {
        let registration = registration(&json!([{"entities": [{"type": VEHICLE}]}]));
        let fragment = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep).as_fragment();
        assert!(fragment.get("id").is_none());
        assert!(fragment.get("type").is_none());
        assert!(fragment.get("entities").is_some());
    }

    #[test]
    fn a_registration_that_covers_none_of_the_selectors_yields_no_entities_member() {
        let registration = registration(&json!([{"entities": [{"type": "https://example.org/Sensor"}]}]));
        let document = ReducedSubscription::reduce(&subscription(), &registration, &receiver(), &minted(), FilterCarriage::Keep).document;
        assert!(document.get("entities").is_none());
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Split), FilterCarriage::Strip);
        pretty_assertions::assert_eq!(FilterCarriage::for_split(SplitEntities::Complete), FilterCarriage::Keep);
    }
}
