//! The entities a periodic (`timeInterval`) notification considers (clause
//! 5.8.6): those its subscription's entity selectors select, read from the
//! store a page at a time.

use crate::error::NotificationError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::payload_pages::EntityPayloadPages;
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
                    // clone: the set owns the identifiers already selected
                    selector.selects(entity.types().as_slice(), entity.id()) && selected.insert(entity.id().clone())
                })
                .await?;
            candidates.extend(page_candidates);
        }

        Ok(candidates)
    }

    /// Read entities a page at a time, keeping only those `keep` accepts.
    ///
    /// `entity_type` narrows the read to one entity type when the selector names
    /// exactly one; otherwise every entity is a candidate and the predicate is
    /// the only filter. See [`INTERVAL_ENTITY_PAGE`] for why this pages.
    async fn collect_pages(
        &self,
        entity_type: Option<&EntityTypeName>,
        mut keep: impl FnMut(&EntityState) -> bool,
    ) -> Result<Vec<EntityState>, NotificationError> {
        let limit = i64::from(INTERVAL_ENTITY_PAGE);
        let mut kept = Vec::new();
        let mut offset = 0_i64;
        loop {
            let page = match entity_type {
                Some(type_name) => self
                    .pages
                    .get_entities_by_type(self.target, type_name, Some(limit), Some(offset))
                    .await
                    .map_err(NotificationError::from)?,
                None => self
                    .pages
                    .get_all_entities(self.target, Some(limit), Some(offset))
                    .await
                    .map_err(NotificationError::from)?,
            };
            let exhausted = page.len() < usize::from(INTERVAL_ENTITY_PAGE);
            kept.extend(page.into_iter().filter(|entity| keep(entity)));
            if exhausted {
                return Ok(kept);
            }
            offset += limit;
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::interval_candidates::IntervalCandidates;
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{error::EntityError, port::payload_pages::EntityPayloadPages};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
    use andromeda_subscription::{compilation::compile_subscription, stored_subscription::StoredSubscription};
    use async_trait::async_trait;
    use serde_json::json;

    const VEHICLE: &str = "https://example.org/Vehicle";

    /// The stored entities, served whole as one page.
    struct StoredEntities(Vec<EntityState>);

    #[async_trait]
    impl EntityPayloadPages for StoredEntities {
        async fn get_entities_by_type(
            &self,
            _target: &ContextTarget,
            entity_type: &EntityTypeName,
            _limit: Option<i64>,
            _offset: Option<i64>,
        ) -> Result<Vec<EntityState>, EntityError> {
            Ok(self
                .0
                .iter()
                .filter(|entity| entity.types().as_slice().contains(entity_type))
                .cloned() // clone: each read hands out its own states
                .collect())
        }

        async fn get_all_entities(&self, _target: &ContextTarget, _limit: Option<i64>, _offset: Option<i64>) -> Result<Vec<EntityState>, EntityError> {
            Ok(self.0.clone()) // clone: each read hands out its own states
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
            },
            LanguageMatching::TagEquality,
        );
        let pages = StoredEntities(vec![vehicle("urn:ngsi-ld:Vehicle:1"), vehicle("urn:ngsi-ld:Vehicle:2")]);
        let candidates = IntervalCandidates {
            target: &ContextTarget::Live,
            pages: &pages,
        };

        let matched = candidates.matching(&compiled).await.expect("the pages read");

        pretty_assertions::assert_eq!(matched, vec![vehicle("urn:ngsi-ld:Vehicle:2"), vehicle("urn:ngsi-ld:Vehicle:1")]);
    }
}
