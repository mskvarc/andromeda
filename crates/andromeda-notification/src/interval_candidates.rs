//! The entities a periodic (`timeInterval`) notification considers (clause
//! 5.8.6): those its subscription's entity selectors select, read from the
//! store a page at a time.

use crate::error::NotificationError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::{payload_pages::EntityPayloadPages, stored_entry::StoredEntry};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_subscription::compiled::CompiledSubscription;
use std::collections::HashSet;

/// Rows read per round trip while a periodic notification assembles its
/// candidate set.
///
/// A selector whose type is a boolean expression cannot be pushed into a
/// `WHERE`, so the broker reads every entity in the tenant and decides in
/// memory. Reading that in pages bounds one round trip, and discarding
/// non-matching rows as each page arrives is what bounds what is held.
const INTERVAL_ENTITY_PAGE: u16 = 500;

/// The candidate entities of one periodic notification, read from `pages`.
pub(crate) struct IntervalCandidates<'a> {
    /// The context the entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// The store's paged entity reads.
    pub(crate) pages: &'a dyn EntityPayloadPages,
}

impl IntervalCandidates<'_> {
    /// The entities `sub`'s entity selectors select (clause 5.8.6), every
    /// entity when it names none.
    ///
    /// The selectors are a union: an entity two of them select is one
    /// candidate, in the place the first selector that selects it puts it.
    ///
    /// # Errors
    ///
    /// Returns the store's failure to read a page.
    pub(crate) async fn matching(&self, sub: &CompiledSubscription) -> Result<Vec<EntityState>, NotificationError> {
        if sub.selectors.is_empty() {
            return self.collect_pages(None, |_| true).await;
        }

        let mut selected: HashSet<Identifier> = HashSet::new();
        let mut candidates = Vec::new();
        for selector in &sub.selectors {
            // A by-type lookup takes one type, so only a leaf selector can be
            // pushed down; a boolean expression reads every entity and is
            // applied in memory.
            let page_candidates = self
                .collect_pages(selector.type_.as_single_type(), |entity| {
                    selector.selects(entity.types().as_slice(), entity.id()) && selected.insert(entity.id().clone())
                })
                .await?;
            candidates.extend(page_candidates);
        }

        Ok(candidates)
    }

    /// Read entities a page at a time, each page continuing after the last
    /// identifier of the one before, keeping only those `keep` accepts. An
    /// entity whose stored rows no longer read back is left out, and the walk
    /// continues after it.
    ///
    /// `entity_type` narrows the read to one entity type when the selector names
    /// exactly one; otherwise every entity is a candidate and the predicate is
    /// the only filter. See [`INTERVAL_ENTITY_PAGE`] for why this pages.
    async fn collect_pages(
        &self,
        entity_type: Option<&EntityTypeName>,
        mut keep: impl FnMut(&EntityState) -> bool,
    ) -> Result<Vec<EntityState>, NotificationError> {
        let mut kept = Vec::new();
        let mut after: Option<Identifier> = None;
        loop {
            let page = match entity_type {
                Some(type_name) => self
                    .pages
                    .get_entities_by_type(self.target, type_name, after.as_ref(), INTERVAL_ENTITY_PAGE)
                    .await
                    .map_err(NotificationError::from)?,
                None => self
                    .pages
                    .get_all_entities(self.target, after.as_ref(), INTERVAL_ENTITY_PAGE)
                    .await
                    .map_err(NotificationError::from)?,
            };
            let exhausted = page.len() < usize::from(INTERVAL_ENTITY_PAGE);
            // clone: the page is consumed by the filter that follows, so the cursor keeps its own copy of the last identifier
            after = page.last().map(|entry| entry.id(EntityState::id).clone());
            // The store logged each entity that does not read back; the others
            // are still notified.
            kept.extend(page.into_iter().filter_map(StoredEntry::read).filter(|entity| keep(entity)));
            if exhausted {
                return Ok(kept);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::interval_candidates::IntervalCandidates;
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        error::EntityError,
        port::{payload_pages::EntityPayloadPages, stored_entry::StoredEntry},
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
    use andromeda_subscription::{
        compilation::compile_subscription,
        compiled::CompiledSubscription,
        notifying_since::NotifyingSince,
        stored_subscription::StoredSubscription,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::Mutex;

    const VEHICLE: &str = "https://example.org/Vehicle";

    /// The stored entities in identifier order, served a page at a time, with
    /// the `after` of every read noted.
    struct StoredEntities {
        entities: Vec<StoredEntry<EntityState>>,
        reads: Mutex<Vec<Option<Identifier>>>,
    }

    impl StoredEntities {
        fn of(entities: Vec<EntityState>) -> StoredEntities {
            StoredEntities::with_entries(entities.into_iter().map(StoredEntry::Read).collect())
        }

        fn with_entries(entities: Vec<StoredEntry<EntityState>>) -> StoredEntities {
            StoredEntities {
                entities,
                reads: Mutex::new(Vec::new()),
            }
        }

        fn page(&self, entity_type: Option<&EntityTypeName>, after: Option<&Identifier>, limit: u16) -> Vec<StoredEntry<EntityState>> {
            self.reads.lock().expect("the notes are not poisoned").push(after.cloned());
            self.entities
                .iter()
                .filter(|entry| match entry {
                    StoredEntry::Read(entity) => entity_type.is_none_or(|wanted| entity.types().as_slice().contains(wanted)),
                    StoredEntry::Unreadable(_) => true,
                })
                .filter(|entry| after.is_none_or(|after| entry.id(EntityState::id).as_str() > after.as_str()))
                .take(usize::from(limit))
                .cloned()
                .collect()
        }
    }

    #[async_trait]
    impl EntityPayloadPages for StoredEntities {
        async fn get_entities_by_type(
            &self,
            _target: &ContextTarget,
            entity_type: &EntityTypeName,
            after: Option<&Identifier>,
            limit: u16,
        ) -> Result<Vec<StoredEntry<EntityState>>, EntityError> {
            Ok(self.page(Some(entity_type), after, limit))
        }

        async fn get_all_entities(
            &self,
            _target: &ContextTarget,
            after: Option<&Identifier>,
            limit: u16,
        ) -> Result<Vec<StoredEntry<EntityState>>, EntityError> {
            Ok(self.page(None, after, limit))
        }
    }

    fn vehicle(id: &str) -> EntityState {
        let document = json!([{"@id": id, "@type": [VEHICLE]}]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    /// Clause 5.8.6 notifies the entities the selectors select, a union of
    /// selectors (clause 5.2.33), so an entity both select is carried once.
    #[tokio::test]
    async fn an_entity_two_selectors_select_is_a_periodic_candidate_once() {
        let subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:fleet",
            "type": "Subscription",
            "entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:2"}, {"type": VEHICLE}],
            "timeInterval": 60,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        let compiled = compile_subscription(
            &StoredSubscription {
                key: SubscriptionKey::try_from(1).expect("a valid key"),
                subscription,
                notifying_since: NotifyingSince {
                    active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                    selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                },
            },
            LanguageMatching::TagEquality,
        );
        let pages = StoredEntities::of(vec![vehicle("urn:ngsi-ld:Vehicle:1"), vehicle("urn:ngsi-ld:Vehicle:2")]);
        let candidates = IntervalCandidates {
            target: &ContextTarget::Live,
            pages: &pages,
        };

        let matched = candidates.matching(&compiled).await.expect("the pages read");

        pretty_assertions::assert_eq!(matched, vec![vehicle("urn:ngsi-ld:Vehicle:2"), vehicle("urn:ngsi-ld:Vehicle:1")]);
    }

    /// A periodic subscription on every Vehicle.
    fn all_vehicles() -> CompiledSubscription {
        let subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:fleet",
            "type": "Subscription",
            "entities": [{"type": VEHICLE}],
            "timeInterval": 60,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        compile_subscription(
            &StoredSubscription {
                key: SubscriptionKey::try_from(1).expect("a valid key"),
                subscription,
                notifying_since: NotifyingSince {
                    active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                    selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                },
            },
            LanguageMatching::TagEquality,
        )
    }

    /// Done when: the walk continues after the last identifier of the previous
    /// page, never by offset, and reads every entity once.
    #[tokio::test]
    async fn a_walk_over_several_pages_continues_after_each_last_identifier() {
        let ids: Vec<String> = (0..1_100).map(|n| format!("urn:ngsi-ld:Vehicle:{n:05}")).collect();
        let pages = StoredEntities::of(ids.iter().map(|id| vehicle(id)).collect());
        let candidates = IntervalCandidates {
            target: &ContextTarget::Live,
            pages: &pages,
        };

        let matched = candidates.matching(&all_vehicles()).await.expect("the pages read");

        let matched_ids: Vec<&str> = matched.iter().map(|entity| entity.id().as_str()).collect();
        pretty_assertions::assert_eq!(matched_ids, ids.iter().map(String::as_str).collect::<Vec<_>>());
        let reads = pages.reads.lock().expect("the notes are not poisoned");
        let afters: Vec<Option<&str>> = reads.iter().map(|after| after.as_ref().map(Identifier::as_str)).collect();
        pretty_assertions::assert_eq!(afters, vec![None, Some("urn:ngsi-ld:Vehicle:00499"), Some("urn:ngsi-ld:Vehicle:00999")]);
    }

    /// An entity stored in a form the broker no longer reads, last on its
    /// page, is left out, and the next page still continues after it.
    #[tokio::test]
    async fn an_unreadable_entity_is_left_out_and_the_walk_continues_after_it() {
        let ids: Vec<String> = (0..600).map(|n| format!("urn:ngsi-ld:Vehicle:{n:05}")).collect();
        let unreadable = "urn:ngsi-ld:Vehicle:00499";
        let pages = StoredEntities::with_entries(
            ids.iter()
                .map(|id| match id.as_str() {
                    raw if raw == unreadable => StoredEntry::Unreadable(raw.parse().expect("a valid URI")),
                    readable => StoredEntry::Read(vehicle(readable)),
                })
                .collect(),
        );
        let candidates = IntervalCandidates {
            target: &ContextTarget::Live,
            pages: &pages,
        };

        let matched = candidates.matching(&all_vehicles()).await.expect("the pages read");

        let matched_ids: Vec<&str> = matched.iter().map(|entity| entity.id().as_str()).collect();
        let expected: Vec<&str> = ids.iter().map(String::as_str).filter(|id| *id != unreadable).collect();
        pretty_assertions::assert_eq!(matched_ids, expected);
        let reads = pages.reads.lock().expect("the notes are not poisoned");
        let afters: Vec<Option<&str>> = reads.iter().map(|after| after.as_ref().map(Identifier::as_str)).collect();
        pretty_assertions::assert_eq!(afters, vec![None, Some(unreadable)]);
    }
}
