//! The entities a periodic (`timeInterval`) notification considers (clause
//! 5.8.6): those the query of its subscription matches as they stand at the
//! tick's instant, read from the store a page at a time.

use crate::{error::NotificationError, interval_query::interval_query};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{port::stored_entry::StoredEntry, query::EntityQueryService};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_query::page_window::{PageStart, PageWindow};
use andromeda_subscription::compiled::CompiledSubscription;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Entities read per round trip while a periodic notification assembles its
/// candidate set.
///
/// The store keeps only the Entities the subscription's selectors and filters
/// select, which can still be every Entity in the tenant. Reading them in
/// pages bounds one round trip and what one read holds.
const INTERVAL_ENTITY_PAGE: u16 = 500;

/// The candidate entities of one periodic notification, read through
/// `entity_query`.
pub(crate) struct IntervalCandidates<'a> {
    /// The context the entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// The query engine the entities are read through.
    pub(crate) entity_query: &'a EntityQueryService,
}

impl IntervalCandidates<'_> {
    /// The entities `sub` notifies of as they stand at `at` (clause 5.8.6), in
    /// identifier order: those its selectors select and its `q`, `geoQ` and
    /// `scopeQ` keep, every entity when it names none, and none when a stored
    /// `geoQ` no longer compiles.
    ///
    /// Each page continues after the last identifier of the one before, never
    /// by offset. An entity whose stored rows no longer read back is left out,
    /// and the walk continues after it.
    ///
    /// # Errors
    ///
    /// Returns the store's failure to read a page.
    pub(crate) async fn matching(&self, sub: &CompiledSubscription, at: DateTime<Utc>) -> Result<Vec<EntityState>, NotificationError> {
        let Some(query) = interval_query(sub, at) else {
            return Ok(Vec::new());
        };
        let query = Arc::new(query);
        let mut kept = Vec::new();
        let mut window = PageWindow {
            limit: u32::from(INTERVAL_ENTITY_PAGE),
            start: PageStart::Cursor(None),
        };
        loop {
            let page = self.entity_query.compiled_states(self.target, &query, &window).await?;
            let exhausted = page.len() < usize::from(INTERVAL_ENTITY_PAGE);
            // clone: the page is consumed by the filter that follows, so the cursor keeps its own copy of the last identifier
            let last = page.last().map(|entry| entry.id(|entity| entity.state.id()).clone());
            // The store logged each entity that does not read back; the others
            // are still notified.
            kept.extend(page.into_iter().filter_map(StoredEntry::read).map(|entity| Arc::unwrap_or_clone(entity.state)));
            match last {
                Some(last) if !exhausted => window = PageWindow::after(u32::from(INTERVAL_ENTITY_PAGE), last),
                Some(_) | None => return Ok(kept),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        interval_candidates::IntervalCandidates,
        notifier_test_double::entity_query,
        periodic_fixture::{VEHICLE, periodic, tick},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        error::EntityError,
        port::{query::EntityQueryStore, retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
        query_outcome::QueryOutcome,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        entity::compiled::CompiledEntityQuery,
        page_window::{PageStart, PageWindow},
        standing_instant::StandingInstant,
    };
    use andromeda_subscription::compiled::CompiledSubscription;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// The stored entities in identifier order, served a window at a time as
    /// a query engine serves the matches of a cursor walk, with the window
    /// and the instant of every read noted.
    struct StoredMatches {
        entities: Vec<StoredEntry<EntityState>>,
        reads: Mutex<Vec<(PageStart, StandingInstant)>>,
    }

    impl StoredMatches {
        fn with_entries(entities: Vec<StoredEntry<EntityState>>) -> Arc<StoredMatches> {
            Arc::new(StoredMatches {
                entities,
                reads: Mutex::new(Vec::new()),
            })
        }

        fn reads(&self) -> Vec<(PageStart, StandingInstant)> {
            self.reads.lock().expect("the notes are not poisoned").clone()
        }
    }

    #[async_trait]
    impl EntityQueryStore for StoredMatches {
        async fn query_compiled(
            &self,
            _target: &ContextTarget,
            compiled: &CompiledEntityQuery,
            window: &PageWindow,
            _count: CountPolicy,
        ) -> Result<QueryOutcome, EntityError> {
            self.reads
                .lock()
                .expect("the notes are not poisoned")
                .push((window.start.clone(), compiled.selection.standing));
            let rows = window
                .cut(self.entities.clone(), |entry| entry.id(EntityState::id).as_str())
                .into_iter()
                .map(|entry| {
                    entry.map(|state| RetrievedEntity {
                        state: Arc::new(state),
                        sys_attrs: SysAttrsSnapshot {
                            created_at: DateTime::<Utc>::MIN_UTC,
                            modified_at: DateTime::<Utc>::MIN_UTC,
                            expires_at: None,
                        },
                    })
                })
                .collect();
            Ok(QueryOutcome { rows, total_count: None })
        }

        async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery, _window: &PageWindow) -> Result<Vec<Identifier>, EntityError> {
            Ok(Vec::new())
        }
    }

    fn vehicle(id: &str) -> EntityState {
        let document = json!([{"@id": id, "@type": [VEHICLE]}]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    fn vehicles(count: usize) -> Vec<String> {
        (0..count).map(|n| format!("urn:ngsi-ld:Vehicle:{n:05}")).collect()
    }

    fn every_vehicle() -> CompiledSubscription {
        periodic(&json!({"entities": [{"type": VEHICLE}]}))
    }

    fn after(id: &str) -> PageStart {
        PageStart::Cursor(Some(PageCursor::After(id.parse().expect("a valid URI"))))
    }

    async fn matched_ids(store: &Arc<StoredMatches>, sub: &CompiledSubscription) -> Vec<String> {
        let service = entity_query(Arc::<StoredMatches>::clone(store));
        let candidates = IntervalCandidates {
            target: &ContextTarget::Live,
            entity_query: &service,
        };
        let matched = candidates.matching(sub, tick()).await.expect("the pages read");
        matched.iter().map(|entity| entity.id().as_str().to_owned()).collect()
    }

    /// The walk starts at the first identifier and continues after the last
    /// identifier of each full page, never by offset, reading every match
    /// once, each page at the tick's one instant.
    #[tokio::test]
    async fn a_walk_over_several_pages_continues_after_each_last_identifier_at_the_tick_instant() {
        let ids = vehicles(1_100);
        let store = StoredMatches::with_entries(ids.iter().map(|id| StoredEntry::Read(vehicle(id))).collect());

        let matched = matched_ids(&store, &every_vehicle()).await;

        assert_eq!(matched, ids);
        let at = StandingInstant::At(tick());
        assert_eq!(
            store.reads(),
            vec![
                (PageStart::Cursor(None), at),
                (after("urn:ngsi-ld:Vehicle:00499"), at),
                (after("urn:ngsi-ld:Vehicle:00999"), at),
            ]
        );
    }

    /// An entity stored in a form the broker no longer reads, last on its
    /// page, is left out, and the next page still continues after it.
    #[tokio::test]
    async fn an_unreadable_entity_is_left_out_and_the_walk_continues_after_it() {
        let ids = vehicles(600);
        let unreadable = "urn:ngsi-ld:Vehicle:00499";
        let store = StoredMatches::with_entries(
            ids.iter()
                .map(|id| match id.as_str() {
                    raw if raw == unreadable => StoredEntry::Unreadable(raw.parse().expect("a valid URI")),
                    readable => StoredEntry::Read(vehicle(readable)),
                })
                .collect(),
        );

        let matched = matched_ids(&store, &every_vehicle()).await;

        let expected: Vec<String> = ids.into_iter().filter(|id| id != unreadable).collect();
        assert_eq!(matched, expected);
        let reads: Vec<PageStart> = store.reads().into_iter().map(|(start, _)| start).collect();
        assert_eq!(reads, vec![PageStart::Cursor(None), after(unreadable)]);
    }

    /// Clause 5.8.6: a stored `geoQ` that no longer compiles is a defined
    /// geoquery, so the notification reads no Entity at all.
    #[tokio::test]
    async fn a_geo_q_that_no_longer_compiles_reads_no_page() {
        let store = StoredMatches::with_entries(vehicles(3).iter().map(|id| StoredEntry::Read(vehicle(id))).collect());
        let unusable = periodic(&json!({
            "entities": [{"type": VEHICLE}],
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": "[1, "}
        }));

        let matched = matched_ids(&store, &unusable).await;

        assert_eq!((matched, store.reads()), (Vec::new(), Vec::new()));
    }
}
