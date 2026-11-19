//! Spelling a stored subscription in the terms of the reader's `@context`
//! (ETSI GS CIM 009 clause 5.5.7) and as it stands at read time.
//!
//! Everything a subscription is matched by is stored expanded, so serving it
//! back compacts each name against the `@context` of the request that reads it.
//! The members that decide how the resource reads are left as the store holds
//! them: the transport writes the clause 5.2.12 document, and the projection
//! lists it writes as the clause 4.21 strings they were sent as.

use crate::subscription_invariant::SubscriptionInvariant;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use andromeda_ngsi_ld_model::{
    entity_selector::EntitySelector,
    subscription::Subscription,
    subscription_presentation::SubscriptionPresentation,
    written_subscription_terms::WrittenSubscriptionTerms,
};
use andromeda_ngsi_ld_query::{
    filter::{
        Filter,
        compaction::{compact_context_source_filter, compact_filter, revert_expand_values},
    },
    geo::geo_constraint::GeoConstraint,
    name_stage::{Expanded, Written},
    projection::{
        expanded_projection_term::ExpandedProjectionTerm,
        written_projection_list::WrittenProjectionList,
        written_projection_term::WrittenProjectionTerm,
    },
    type_selection::compaction::compact_type_selection,
};
use chrono::Utc;
use std::collections::HashSet;

/// Report a subscription whose expiration is due as expired (clause 5.2.12).
fn report_expired_status(subscription: &mut Subscription) {
    let standing = &mut subscription.lifecycle.standing;
    standing.status = standing.reported_status(Utc::now());
}

/// A name in the reader's terms, or as it stands when no term of the `@context`
/// spells it.
fn compacted_name(compactor: &NameCompaction, name: AttributeName) -> AttributeName {
    compactor.compact_attr_iri(name.as_str()).parse().unwrap_or(name)
}

/// The names in the reader's terms.
fn compacted_names(compactor: &NameCompaction, names: Vec<AttributeName>) -> Vec<AttributeName> {
    names.into_iter().map(|name| compacted_name(compactor, name)).collect()
}

/// A selector whose every type of the stored selection is in the reader's
/// terms (clause 4.17), a type no term of the `@context` spells staying as
/// stored. The selector as it stands when the selection cannot be compacted.
async fn compacted_selector(compactor: &NameCompaction, stored: &EntitySelector<Expanded>, mut spelled: EntitySelector<Written>) -> EntitySelector<Written> {
    if let Ok(type_) = compact_type_selection(&stored.type_, compactor).await {
        spelled.type_ = type_;
    }
    spelled
}

/// The entity selectors in the reader's terms.
async fn compacted_selectors(
    compactor: &NameCompaction,
    stored: &[EntitySelector<Expanded>],
    spelled: Vec<EntitySelector<Written>>,
) -> Vec<EntitySelector<Written>> {
    let mut selectors = Vec::with_capacity(spelled.len());
    for (stored, spelled) in stored.iter().zip(spelled) {
        selectors.push(compacted_selector(compactor, stored, spelled).await);
    }
    selectors
}

/// A projection term, and the terms linked under it, in the reader's terms. The
/// clause 4.21 keywords are no attribute names and stay.
fn compacted_term(compactor: &NameCompaction, term: WrittenProjectionTerm) -> WrittenProjectionTerm {
    let name = match ExpandedProjectionTerm::keyword(term.name.as_str()) {
        Some(_) => term.name,
        None => compactor.compact_attr_iri(term.name.as_str()).parse().unwrap_or(term.name),
    };
    WrittenProjectionTerm {
        name,
        linked: term.linked.into_iter().map(|linked| compacted_term(compactor, linked)).collect(),
    }
}

/// The projection lists in the reader's terms.
fn compacted_projection(compactor: &NameCompaction, lists: Vec<WrittenProjectionList>) -> Vec<WrittenProjectionList> {
    lists
        .into_iter()
        .map(|list| {
            Vec::<WrittenProjectionTerm>::from(list)
                .into_iter()
                .map(|term| compacted_term(compactor, term))
                .collect::<Vec<_>>()
                .into()
        })
        .collect()
}

/// The stored `q` in the reader's terms, with the `expandValues` coercion
/// reverted, or as it stands when it cannot be compacted.
async fn compacted_q(
    compactor: &NameCompaction,
    stored: &Filter<Expanded>,
    opted_in: &HashSet<AttributeIri>,
    spelled: Option<Filter<Written>>,
) -> Option<Filter<Written>> {
    // clone: reverting consumes the tree and the stored subscription keeps its own
    let reverted = revert_expand_values(stored.clone(), opted_in, compactor).await;
    compact_filter(reverted, compactor).await.ok().or(spelled)
}

/// The stored `csf` in the reader's terms, or as it stands when it cannot be
/// compacted.
async fn compacted_csf(compactor: &NameCompaction, stored: &Filter<Expanded>, spelled: Option<Filter<Written>>) -> Option<Filter<Written>> {
    // clone: compaction consumes the tree and the stored subscription keeps its own
    compact_context_source_filter(stored.clone(), compactor).await.ok().or(spelled)
}

/// A geo-query whose geoproperty, when it names one, is in the reader's terms
/// (clause 5.2.13). An absent geoproperty means `location` and stays absent.
fn compacted_geo_q(compactor: &NameCompaction, mut geo_q: GeoConstraint<Written>) -> GeoConstraint<Written> {
    geo_q.geoproperty = geo_q.geoproperty.map(|name| compacted_name(compactor, name));
    geo_q
}

/// The names of a stored subscription in the reader's terms.
async fn compacted_terms(compactor: &NameCompaction, stored: &Subscription, spelled: WrittenSubscriptionTerms) -> WrittenSubscriptionTerms {
    let filters = &stored.filters;
    let opted_in: HashSet<AttributeIri> = filters.expand_values.iter().flatten().cloned().collect();
    let q = match &filters.q {
        Some(q) => compacted_q(compactor, q, &opted_in, spelled.q).await,
        None => None,
    };
    let csf = match &filters.csf {
        Some(csf) => compacted_csf(compactor, csf, spelled.csf).await,
        None => None,
    };
    let entities = match (&stored.selection.entities, spelled.entities) {
        (Some(stored), Some(spelled)) => Some(compacted_selectors(compactor, stored, spelled).await),
        (None, spelled) => spelled,
        (Some(_), None) => None,
    };
    WrittenSubscriptionTerms {
        entities,
        watched_attributes: spelled.watched_attributes.map(|names| compacted_names(compactor, names)),
        attributes: spelled.attributes.map(|names| compacted_names(compactor, names)),
        pick: spelled.pick.map(|lists| compacted_projection(compactor, lists)),
        omit: spelled.omit.map(|lists| compacted_projection(compactor, lists)),
        q,
        geo_q: spelled.geo_q.map(|geo_q| compacted_geo_q(compactor, geo_q)),
        csf,
        expand_values: spelled.expand_values.map(|names| compacted_names(compactor, names)),
        json_keys: spelled.json_keys.map(|names| compacted_names(compactor, names)),
    }
}

/// A stored subscription as its reader reads it: the names in the terms of the
/// reader's `@context`, the expiration reported as it stands now, and
/// `localOnly` only when it is true.
///
/// # Errors
///
/// Returns [`SubscriptionInvariant::StoredNameNotWritable`] when a stored name
/// is past the bound a written name keeps, which the store never holds.
pub(crate) async fn compact_for_response(
    mut subscription: Subscription,
    compactor: &NameCompaction,
) -> Result<SubscriptionPresentation, SubscriptionInvariant> {
    report_expired_status(&mut subscription);
    subscription.delivery.local_only = subscription.delivery.local_only.filter(|local_only| *local_only);
    let spelled = WrittenSubscriptionTerms::try_from(&subscription)?;
    let terms = compacted_terms(compactor, &subscription, spelled).await;
    Ok(SubscriptionPresentation { subscription, terms })
}

#[cfg(test)]
mod tests {
    use crate::response_compaction::compact_for_response;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::rendering::instant_text};
    use andromeda_ngsi_ld_model::{
        subscription::{Subscription, SubscriptionStatus},
        subscription_presentation::SubscriptionPresentation,
    };
    use chrono::{Duration, Utc};
    use serde_json::{Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The names of a reader that supplied no `@context` of its own.
    async fn core_names() -> NameCompaction {
        let processed = offline_engine()
            .compactor(&ContextSpec::none())
            .processed_context()
            .await
            .expect("the core context processes");
        NameCompaction::new(processed)
    }

    /// A stored Subscription whose members are `members`, over the minimum a
    /// Subscription holds.
    fn stored(members: &Value) -> Subscription {
        let mut wire = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(extra)) = (wire.as_object_mut(), members.as_object()) {
            target.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        serde_json::from_value(wire).expect("a valid subscription")
    }

    /// The Subscription as the reader reads it, written as the document it is.
    async fn served(members: &Value) -> Value {
        let terms = presented(members).await.terms;
        let comma_separated = |names: Option<Vec<AttributeName>>| names.map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(","));
        json!({
            "entities": terms.entities,
            "watchedAttributes": terms.watched_attributes,
            "q": terms.q.map(|q| q.to_string()),
            "csf": terms.csf.map(|csf| csf.to_string()),
            "expandValues": comma_separated(terms.expand_values),
            "jsonKeys": comma_separated(terms.json_keys)
        })
    }

    /// The Subscription of `members` as the reader of the core `@context` reads it.
    async fn presented(members: &Value) -> SubscriptionPresentation {
        compact_for_response(stored(members), &core_names().await)
            .await
            .expect("the stored names are writable")
    }

    /// Clause 5.8.1.4: a subscription whose `expiresAt` is due reports
    /// `expired` on read, whatever status the row still carries, and the
    /// client's `isActive` is not rewritten.
    #[tokio::test]
    async fn a_due_expiration_reports_status_expired_at_read_time() {
        let due = instant_text(Utc::now() - Duration::seconds(1));
        let subscription = presented(&json!({"expiresAt": due, "isActive": true})).await.subscription;

        pretty_assertions::assert_eq!(subscription.lifecycle.standing.status, SubscriptionStatus::Expired);
        pretty_assertions::assert_eq!(subscription.lifecycle.is_active, Some(true));
    }

    #[tokio::test]
    async fn a_future_or_absent_expiration_leaves_the_status_alone() {
        let future = instant_text(Utc::now() + Duration::hours(1));
        let subscription = presented(&json!({"expiresAt": future})).await.subscription;
        pretty_assertions::assert_eq!(subscription.lifecycle.standing.status, SubscriptionStatus::Active);

        let perpetual = presented(&json!({"status": "paused"})).await.subscription;
        pretty_assertions::assert_eq!(perpetual.lifecycle.standing.status, SubscriptionStatus::Paused);
    }

    /// Table 5.2.12-1 defaults `localOnly` to false, so only `true` is served.
    #[tokio::test]
    async fn local_only_is_served_only_when_it_is_true() {
        let local = presented(&json!({"localOnly": true})).await.subscription;
        pretty_assertions::assert_eq!(local.delivery.local_only, Some(true));
        let explicit_default = presented(&json!({"localOnly": false})).await.subscription;
        pretty_assertions::assert_eq!(explicit_default.delivery.local_only, None);
    }

    /// Clause 5.5.7: a stored `q` and `csf` read back in the reader's terms.
    #[tokio::test]
    async fn stored_filters_are_compacted_into_the_readers_terms() {
        let served = served(&json!({
            "q": "\"https://uri.etsi.org/ngsi-ld/default-context/speed\">10",
            "csf": "\"https://uri.etsi.org/ngsi-ld/default-context/tier\"==\"gold\""
        }))
        .await;

        pretty_assertions::assert_eq!(served["q"], json!("speed>10"));
        pretty_assertions::assert_eq!(served["csf"], json!("tier==\"gold\""));
    }

    /// Clause 4.9: a stored `csf` member is the literal key of a Context Source
    /// Property's raw JSON value, so it reads back exactly as written even when
    /// it is spelled like an IRI the reader's terms would abbreviate.
    #[tokio::test]
    async fn a_stored_csf_reads_back_with_its_trailing_members_as_written() {
        let served = served(&json!({
            "csf": "\"https://uri.etsi.org/ngsi-ld/default-context/coverage\"[\"https://uri.etsi.org/ngsi-ld/default-context/region\"]==\"north\""
        }))
        .await;

        pretty_assertions::assert_eq!(
            served["csf"],
            json!("coverage[\"https://uri.etsi.org/ngsi-ld/default-context/region\"]==\"north\"")
        );
    }

    /// Table 5.2.12-1: `expandValues` and `jsonKeys` are stored expanded and
    /// read back in the reader's terms, still as one comma-separated String.
    #[tokio::test]
    async fn the_stored_opt_in_lists_are_compacted_into_the_readers_terms() {
        let served = served(&json!({
            "expandValues": "https://uri.etsi.org/ngsi-ld/default-context/speed,https://uri.etsi.org/ngsi-ld/default-context/colour",
            "jsonKeys": "https://uri.etsi.org/ngsi-ld/default-context/payload"
        }))
        .await;

        pretty_assertions::assert_eq!(served["expandValues"], json!("speed,colour"));
        pretty_assertions::assert_eq!(served["jsonKeys"], json!("payload"));
    }

    /// Clause 5.5.7: the entity types and watched attributes of a subscription
    /// read back in the reader's terms.
    #[tokio::test]
    async fn stored_selectors_and_watched_attributes_are_compacted_into_the_readers_terms() {
        let served = served(&json!({
            "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}],
            "watchedAttributes": ["https://uri.etsi.org/ngsi-ld/default-context/speed"]
        }))
        .await;

        pretty_assertions::assert_eq!(served["entities"], json!([{"type": "Vehicle"}]));
        pretty_assertions::assert_eq!(served["watchedAttributes"], json!(["speed"]));
    }

    /// Clauses 4.17 and 5.5.7: every type of a stored selection reads back in
    /// the reader's terms, in a list of alternatives and in a conjunction.
    #[tokio::test]
    async fn every_type_of_a_stored_type_selection_is_compacted_into_the_readers_terms() {
        let served = served(&json!({
            "entities": [
                {"type": "https://uri.etsi.org/ngsi-ld/default-context/A,https://uri.etsi.org/ngsi-ld/default-context/B"},
                {"type": "(https://uri.etsi.org/ngsi-ld/default-context/A;https://uri.etsi.org/ngsi-ld/default-context/B)"}
            ]
        }))
        .await;

        pretty_assertions::assert_eq!(served["entities"], json!([{"type": "A,B"}, {"type": "(A;B)"}]));
    }

    /// Clauses 5.2.13 and 5.5.7: a stored `geoQ` geoproperty reads back as the
    /// term of the reader's `@context`.
    #[tokio::test]
    async fn a_stored_geo_query_geoproperty_is_compacted_into_the_readers_terms() {
        let geoproperty = |stored: &str| {
            let members = json!({"geoQ": {
                "georel": "within",
                "geometry": "Point",
                "coordinates": [1, 2],
                "geoproperty": stored
            }});
            async move {
                let terms = presented(&members).await.terms;
                terms.geo_q.and_then(|geo_q| geo_q.geoproperty).map(|name| name.as_str().to_owned())
            }
        };

        pretty_assertions::assert_eq!(
            geoproperty("https://uri.etsi.org/ngsi-ld/default-context/footprint").await,
            Some("footprint".to_owned())
        );
        pretty_assertions::assert_eq!(geoproperty("https://uri.etsi.org/ngsi-ld/location").await, Some("location".to_owned()));
    }

    /// Clause 5.5.7: each stored projection tree reads back with its attribute
    /// names compacted at every level and the keywords as written.
    #[tokio::test]
    async fn a_stored_tree_reads_back_with_compacted_names() {
        let subscription = presented(&json!({
            "notification": {
                "endpoint": {"uri": "https://receiver.example.com/notify"},
                "attributes": ["https://uri.etsi.org/ngsi-ld/default-context/speed"],
                "pick": [
                    [{"name": "id", "linked": []}],
                    [{"name": "https://uri.etsi.org/ngsi-ld/default-context/owner", "linked": [
                        {"name": "id", "linked": []},
                        {"name": "https://uri.etsi.org/ngsi-ld/default-context/employer", "linked": [
                            {"name": "https://uri.etsi.org/ngsi-ld/default-context/size", "linked": []}
                        ]}
                    ]}]
                ],
                "omit": [[{"name": "scope", "linked": []}, {"name": "https://uri.etsi.org/ngsi-ld/default-context/colour", "linked": []}]]
            }
        }))
        .await
        .terms;

        let rendered = |lists: Option<Vec<_>>| lists.map(|lists| lists.iter().map(ToString::to_string).collect::<Vec<String>>());
        pretty_assertions::assert_eq!(rendered(subscription.pick), Some(vec!["id".to_owned(), "owner{id,employer{size}}".to_owned()]));
        pretty_assertions::assert_eq!(rendered(subscription.omit), Some(vec!["scope,colour".to_owned()]));
        pretty_assertions::assert_eq!(
            subscription
                .attributes
                .as_deref()
                .map(|names| names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>()),
            Some(vec!["speed".to_owned()])
        );
    }
}
