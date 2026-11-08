//! Spelling a stored subscription in the terms of the reader's `@context`
//! (ETSI GS CIM 009 clause 5.5.7) and as it stands at read time.
//!
//! Everything a subscription is matched by is stored expanded, so serving it
//! back compacts each name against the `@context` of the request that reads it.
//! The members that decide how the resource reads are left as the store holds
//! them: the transport writes the clause 5.2.12 document, and the projection
//! lists it writes as the clause 4.21 strings they were sent as.

use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::subscription::{NotificationParams, Subscription, SubscriptionFilters, entity_selector::EntitySelector};
use andromeda_ngsi_ld_query::{
    filter::compaction::{compact_csf_expr, compact_q_expr, revert_expand_values},
    projection::{
        grammar::{ProjectionList, RawProjectionTerm},
        term::ProjectionTerm,
    },
};
use chrono::Utc;
use std::collections::HashSet;

/// Report the clause 5.2.12 `status` the subscription stands at now.
///
/// The sweeper persisting the clause 5.8.1.4 expiry runs on a cadence, so a
/// read between the due instant and the sweep reports the status already
/// reached rather than the one the row still carries. `isActive` is the
/// client's own member and is left as written.
fn report_expired_status(subscription: &mut Subscription) {
    let standing = &mut subscription.lifecycle.standing;
    standing.status = standing.reported_status(Utc::now());
}

/// One stored attribute name in the reader's terms; a compacted name that is
/// no attribute name is never produced by the compaction, and the expanded one
/// still identifies the attribute.
fn compacted_name(compactor: &NameCompaction, name: AttributeName) -> AttributeName {
    compactor.compact_attr_iri(name.as_str()).parse().unwrap_or(name)
}

/// One stored attribute-name list in the reader's terms.
fn compacted_names(compactor: &NameCompaction, names: Vec<AttributeName>) -> Vec<AttributeName> {
    names.into_iter().map(|name| compacted_name(compactor, name)).collect()
}

/// The type selection of an entity selector in the reader's terms.
fn compacted_selector(compactor: &NameCompaction, mut selector: EntitySelector) -> EntitySelector {
    selector.type_ = compactor.compact_type_iri(&selector.type_.to_string()).parse().unwrap_or(selector.type_);
    selector
}

/// One stored term with every attribute name compacted, at every nesting
/// level; the clause 4.21 keywords name no attribute and stay as written.
fn compacted_term(compactor: &NameCompaction, term: RawProjectionTerm) -> RawProjectionTerm {
    let name = match ProjectionTerm::keyword(term.name.as_str()) {
        Some(_) => term.name,
        None => compactor.compact_attr_iri(term.name.as_str()).parse().unwrap_or(term.name),
    };
    RawProjectionTerm {
        name,
        linked: term.linked.into_iter().map(|linked| compacted_term(compactor, linked)).collect(),
    }
}

/// The stored projection lists of `pick` or `omit` in the reader's terms.
fn compacted_projection(compactor: &NameCompaction, lists: Vec<ProjectionList>) -> Vec<ProjectionList> {
    lists
        .into_iter()
        .map(|list| {
            Vec::<RawProjectionTerm>::from(list)
                .into_iter()
                .map(|term| compacted_term(compactor, term))
                .collect::<Vec<_>>()
                .into()
        })
        .collect()
}

/// The notification parameters with every stored attribute name compacted.
fn compacted_notification(compactor: &NameCompaction, mut notification: NotificationParams) -> NotificationParams {
    notification.attributes = notification.attributes.map(|names| compacted_names(compactor, names));
    notification.pick = notification.pick.map(|lists| compacted_projection(compactor, lists));
    notification.omit = notification.omit.map(|lists| compacted_projection(compactor, lists));
    notification
}

/// Spell the stored `q` and `csf` of a Subscription, and its opt-in lists, in
/// the terms of the reader's `@context` (clause 5.5.7).
///
/// Both filters are clause 4.9 expressions whose attribute terms were expanded
/// when the subscription was written. `scopeQ` is a clause 4.19 scope path and
/// carries no attribute terms, so it is served as stored.
///
/// The `q` operands `expandValues` coerced are put back too. Table 5.2.12-1
/// makes that coercion something done "prior to executing the query", so it
/// belongs to the filter a match runs and not to the Subscription resource: a
/// client that created `q=status=="active"` with `expandValues=status` reads
/// back the filter it wrote. `csf` is exempt because `expandValues` names
/// Entity attributes, while `csf` addresses a registration's Context Source
/// Properties.
async fn compacted_filters(compactor: &NameCompaction, mut filters: SubscriptionFilters) -> SubscriptionFilters {
    if let Some(expr) = filters.q.take() {
        // clone: the opt-in list stays on the resource while the filter is reverted against its names
        let opted_in: HashSet<AttributeName> = filters.expand_values.iter().flatten().cloned().collect();
        let reverted = revert_expand_values(expr, &opted_in, compactor).await;
        filters.q = Some(compact_q_expr(reverted, compactor).await);
    }
    if let Some(expr) = filters.csf.take() {
        filters.csf = Some(compact_csf_expr(expr, compactor).await);
    }
    filters.expand_values = filters.expand_values.map(|names| compacted_names(compactor, names));
    filters.json_keys = filters.json_keys.map(|names| compacted_names(compactor, names));
    filters
}

/// The Subscription as a reader whose names are `compactor`'s reads it: every
/// stored name compacted, a due expiration reported as such, and `localOnly`
/// left absent unless it is `true`, which is the default Table 5.2.12-1 gives
/// the member.
pub(crate) async fn compact_for_response(mut subscription: Subscription, compactor: &NameCompaction) -> Subscription {
    report_expired_status(&mut subscription);
    subscription.delivery.local_only = subscription.delivery.local_only.filter(|local_only| *local_only);
    subscription.selection.entities = subscription
        .selection
        .entities
        .map(|entities| entities.into_iter().map(|selector| compacted_selector(compactor, selector)).collect());
    subscription.selection.watched_attributes = subscription.selection.watched_attributes.map(|names| compacted_names(compactor, names));
    subscription.notification = compacted_notification(compactor, subscription.notification);
    subscription.filters = compacted_filters(compactor, subscription.filters).await;
    subscription
}

#[cfg(test)]
mod tests {
    use crate::response_compaction::compact_for_response;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
    use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
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
        let subscription = compact_for_response(stored(members), &core_names().await).await;
        serde_json::to_value(subscription).expect("the subscription writes")
    }

    /// Clause 5.8.1.4: a subscription whose `expiresAt` is due reports
    /// `expired` on read, whatever status the row still carries, and the
    /// client's `isActive` is not rewritten.
    #[tokio::test]
    async fn a_due_expiration_reports_status_expired_at_read_time() {
        let due = instant_text(Utc::now() - Duration::seconds(1));
        let subscription = compact_for_response(stored(&json!({"expiresAt": due, "isActive": true})), &core_names().await).await;

        pretty_assertions::assert_eq!(subscription.lifecycle.standing.status, SubscriptionStatus::Expired);
        pretty_assertions::assert_eq!(subscription.lifecycle.is_active, Some(true));
    }

    #[tokio::test]
    async fn a_future_or_absent_expiration_leaves_the_status_alone() {
        let future = instant_text(Utc::now() + Duration::hours(1));
        let subscription = compact_for_response(stored(&json!({"expiresAt": future})), &core_names().await).await;
        pretty_assertions::assert_eq!(subscription.lifecycle.standing.status, SubscriptionStatus::Active);

        let perpetual = compact_for_response(stored(&json!({"status": "paused"})), &core_names().await).await;
        pretty_assertions::assert_eq!(perpetual.lifecycle.standing.status, SubscriptionStatus::Paused);
    }

    /// Table 5.2.12-1 defaults `localOnly` to false, so only `true` is served.
    #[tokio::test]
    async fn local_only_is_served_only_when_it_is_true() {
        let names = core_names().await;
        let local = compact_for_response(stored(&json!({"localOnly": true})), &names).await;
        pretty_assertions::assert_eq!(local.delivery.local_only, Some(true));
        let explicit_default = compact_for_response(stored(&json!({"localOnly": false})), &names).await;
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

    /// Clause 5.5.7: each stored projection tree reads back with its attribute
    /// names compacted at every level and the keywords as written.
    #[tokio::test]
    async fn a_stored_tree_reads_back_with_compacted_names() {
        let subscription = compact_for_response(
            stored(&json!({
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
            })),
            &core_names().await,
        )
        .await;

        let rendered = |lists: Option<Vec<_>>| lists.map(|lists| lists.iter().map(ToString::to_string).collect::<Vec<String>>());
        pretty_assertions::assert_eq!(
            rendered(subscription.notification.pick),
            Some(vec!["id".to_owned(), "owner{id,employer{size}}".to_owned()])
        );
        pretty_assertions::assert_eq!(rendered(subscription.notification.omit), Some(vec!["scope,colour".to_owned()]));
        pretty_assertions::assert_eq!(
            subscription
                .notification
                .attributes
                .as_deref()
                .map(|names| names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>()),
            Some(vec!["speed".to_owned()])
        );
    }
}
