//! Gathering the entity states the `rel{…}` hops of an in-memory filter cross
//! into (clause 4.9).
//!
//! A filter matched in memory, a subscription's `q`, a deferred split-query
//! filter, reads the linked entities of its hops from a [`LinkedStates`]
//! filled beforehand. A hop "makes a sub-query on matching target objects"
//! (clause 4.9 EXAMPLE 13), so what has to be read follows from the filter and
//! the entities it is matched against: the objects of each hop's Relationship,
//! then, for a chained hop, the objects the next Relationship names in those
//! entities, and so on. Each level is one batch read.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    filter::{
        expression::Expr,
        hop_steps::{hop_steps, linked_paths},
        path::QueryPath,
    },
    queried_entity::attribute_reading::QueriedEntity,
};
use chrono::{DateTime, Utc};
use std::{borrow::Cow, sync::Arc};

/// One object still to be read, with the path the hop continues along in the
/// entity it names.
type PendingStep<'q> = (Identifier, &'q QueryPath);

impl EntityQueryService {
    /// Read every entity the hops of `filters` reach from `roots`, each as it
    /// stands at `at`.
    ///
    /// A linked entity is read as it currently stands, its instances past their
    /// `expiresAt` left out (clause 4.22), which is how the store's own
    /// compilation of a hop lands on it. An object the store holds no entity
    /// for reaches nothing. Each level continues only along the path left
    /// after a hop, so the levels end with the deepest hop chain of the
    /// filters, a cycle of Relationships included.
    ///
    /// The first level is read off `roots` before anything is awaited, so the
    /// roots need not outlive the call nor be shared across threads.
    ///
    /// # Errors
    ///
    /// Returns the store's [`EntityError`] when a level cannot be read.
    pub fn linked_filter_states<'s, 'a, E: QueriedEntity<'a>>(
        &'s self,
        target: &'s ContextTarget,
        roots: &[E],
        filters: &[&'s Expr],
        at: DateTime<Utc>,
    ) -> impl Future<Output = Result<LinkedStates, EntityError>> + Send + 's {
        let paths: Vec<&QueryPath> = filters.iter().flat_map(|filter| linked_paths(filter)).collect();
        let pending: Vec<PendingStep<'s>> = roots.iter().flat_map(|root| owned_steps(*root, &paths)).collect();
        self.gather_linked(target, pending, at)
    }

    /// Read the entities `pending` names, then every entity the hops continue
    /// into from them, level by level.
    async fn gather_linked(&self, target: &ContextTarget, mut pending: Vec<PendingStep<'_>>, at: DateTime<Utc>) -> Result<LinkedStates, EntityError> {
        let mut linked = LinkedStates::new();
        while !pending.is_empty() {
            let mut unread: Vec<Identifier> = pending.iter().map(|(id, _)| id).filter(|id| !linked.holds(id)).cloned().collect(); // clone: the batch read takes the identifiers the steps keep
            unread.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
            unread.dedup();
            if !unread.is_empty() {
                for (_, retrieved) in self.reader.get_batch(target, &unread).await? {
                    linked.insert(standing(retrieved.state, at));
                }
            }
            pending = pending
                .iter()
                .filter_map(|(id, inner)| Some((linked.get(id)?, *inner)))
                .flat_map(|(state, inner)| owned_steps(state, &[inner]))
                .collect();
        }
        Ok(linked)
    }
}

/// The steps the hops of `paths` take out of `entity`, each object read as the
/// identifier of the entity it names; an object that is no identifier names
/// none (clause 4.5.3).
fn owned_steps<'a, 'q, E: QueriedEntity<'a>>(entity: E, paths: &[&'q QueryPath]) -> Vec<PendingStep<'q>> {
    hop_steps(entity, paths)
        .into_iter()
        .filter_map(|step| Some((step.object.parse::<Identifier>().ok()?, step.inner)))
        .collect()
}

/// The state as it stands at `at`, shared unchanged when no instance of it has
/// expired.
fn standing(state: Arc<EntityState>, at: DateTime<Utc>) -> Arc<EntityState> {
    let expired_removed = match state.standing_at(at) {
        Cow::Owned(standing) => Some(standing),
        Cow::Borrowed(_) => None,
    };
    expired_removed.map_or(state, Arc::new)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        configured_policy::{filter_complexity_limits, geometry_census_limits},
        entity_snapshot::EntitySnapshot,
        error::EntityError,
        port::{query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity},
        query::EntityQueryService,
        query_outcome::QueryOutcome,
        spatial_acceleration::SpatialAcceleration,
        storage_profile::StorageProfile,
    };
    use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        filter::{language_targeting::LanguageTargeting, parser::parse},
        geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
        page_window::PageWindow,
        projection::returned_attributes::ReturnedAttributes,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/Relationship";

    /// A store holding a few entities, recording the batches it was asked for.
    struct Held {
        entities: HashMap<Identifier, Arc<EntityState>>,
        batches: Mutex<Vec<Vec<String>>>,
    }

    fn unread(method: &str) -> EntityError {
        panic!("the linked-state gathering reads through get_batch alone, not {method}")
    }

    #[async_trait]
    impl EntityReader for Held {
        async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
            Err(unread("get"))
        }

        async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
            self.batches
                .lock()
                .expect("the batch log is not poisoned")
                .push(ids.iter().map(|id| id.as_str().to_owned()).collect());
            Ok(ids
                .iter()
                .filter_map(|id| {
                    let state = self.entities.get(id)?;
                    Some((
                        id.clone(),
                        RetrievedEntity {
                            state: Arc::clone(state),
                            sys_attrs: SysAttrsSnapshot {
                                created_at: instant(2024),
                                modified_at: instant(2024),
                                expires_at: None,
                            },
                        },
                    ))
                })
                .collect())
        }

        async fn get_with_sys_attrs(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _returned: &ReturnedAttributes<'_>,
        ) -> Result<Option<RetrievedEntity>, EntityError> {
            Err(unread("get_with_sys_attrs"))
        }

        async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
            Err(unread("get_snapshot"))
        }

        async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
            Err(unread("get_snapshot_batch"))
        }

        async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
            Err(unread("get_sys_attrs"))
        }

        async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
            Err(unread("get_sys_attrs_batch"))
        }
    }

    #[async_trait]
    impl EntityQueryStore for Held {
        fn profile(&self) -> &StorageProfile {
            panic!("the linked-state gathering never inspects the store's profile")
        }

        async fn query_compiled(
            &self,
            _target: &ContextTarget,
            _compiled: &CompiledEntityQuery,
            _window: &PageWindow,
            _count: CountPolicy,
        ) -> Result<QueryOutcome<RetrievedEntity>, EntityError> {
            Err(unread("query_compiled"))
        }

        async fn query_ids(
            &self,
            _target: &ContextTarget,
            _compiled: &CompiledEntityQuery,
            _window: &PageWindow,
        ) -> Result<QueryOutcome<Identifier>, EntityError> {
            Err(unread("query_ids"))
        }
    }

    fn instant(year: i32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).single().expect("a valid instant")
    }

    fn state(document: &Value) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes"))
    }

    /// An entity of type T whose `next` Relationship names `next` and whose
    /// `zone` is `zone`, valid until `zone_expires`.
    fn node(id: &str, next: Option<&str>, zone: &str, zone_expires: Option<&str>) -> Arc<EntityState> {
        let mut document = json!([{
            "@id": id,
            "@type": ["https://example.com/T"],
            "https://example.com/zone": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": zone}]}]
        }]);
        if let Some(expires) = zone_expires {
            document[0]["https://example.com/zone"][0]["https://uri.etsi.org/ngsi-ld/expiresAt"] =
                json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": expires}]);
        }
        if let Some(next) = next {
            document[0]["https://example.com/next"] = json!([{"@type": [RELATIONSHIP], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": next}]}]);
        }
        state(&document)
    }

    fn service(held: &Arc<Held>) -> EntityQueryService {
        EntityQueryService::builder()
            .reader(Arc::clone(held) as Arc<dyn EntityReader>)
            .query_store(Arc::clone(held) as Arc<dyn EntityQueryStore>)
            .engine(JsonLdEngine::builder().allow_remote(false).build().expect("build engine"))
            .spatial(SpatialAcceleration::StoreVerified)
            .tenant(TenantRef::UnnamedDefault)
            .remote_linked(None)
            .geo_admission(GeoQueryAdmission::new(
                geometry_census_limits(&RequestLimitsConfig::default()),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
            .build()
    }

    /// The store holding `a -> b -> c -> a`, the root `r -> a`, each with its
    /// zone.
    fn cycle() -> Arc<Held> {
        let entities = [
            node("urn:a", Some("urn:b"), "north", None),
            node("urn:b", Some("urn:c"), "south", None),
            node("urn:c", Some("urn:a"), "east", Some("2020-01-01T00:00:00Z")),
        ];
        Arc::new(Held {
            entities: entities.into_iter().map(|state| (state.id().clone(), state)).collect(),
            batches: Mutex::new(Vec::new()),
        })
    }

    async fn gather(held: &Arc<Held>, q: &str) -> LinkedStates {
        let root = node("urn:r", Some("urn:a"), "west", None);
        let filter = parse(q).expect("the filter parses");
        service(held)
            .linked_filter_states(&ContextTarget::Live, &[root.as_ref()], &[&filter], instant(2025))
            .await
            .expect("the store answers")
    }

    fn held_ids(linked: &LinkedStates) -> Vec<&str> {
        let mut ids: Vec<&str> = ["urn:a", "urn:b", "urn:c"]
            .into_iter()
            .filter(|id| linked.holds(&id.parse().expect("valid URI")))
            .collect();
        ids.sort_unstable();
        ids
    }

    #[tokio::test]
    async fn the_entities_a_hop_reaches_are_gathered() {
        let held = cycle();
        let linked = gather(&held, "\"https://example.com/next\"{\"https://example.com/zone\"}==\"north\"").await;
        pretty_assertions::assert_eq!(held_ids(&linked), vec!["urn:a"]);
        pretty_assertions::assert_eq!(*held.batches.lock().expect("not poisoned"), vec![vec!["urn:a".to_owned()]]);
    }

    #[tokio::test]
    async fn a_second_hop_is_gathered_from_the_first_level() {
        let held = cycle();
        let linked = gather(
            &held,
            "\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/zone\"}}==\"south\"",
        )
        .await;
        pretty_assertions::assert_eq!(held_ids(&linked), vec!["urn:a", "urn:b"]);
        pretty_assertions::assert_eq!(held.batches.lock().expect("not poisoned").len(), 2, "one batch per level");
    }

    /// `a -> b -> c -> a` is a cycle, yet the gathering ends with the deepest
    /// hop chain the filter writes, and an entity already read is not read
    /// again.
    #[tokio::test]
    async fn a_relationship_cycle_stops_at_the_deepest_hop() {
        let held = cycle();
        let four_hops = "\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/zone\"}}}}";
        let linked = gather(&held, four_hops).await;
        pretty_assertions::assert_eq!(held_ids(&linked), vec!["urn:a", "urn:b", "urn:c"]);
        pretty_assertions::assert_eq!(held.batches.lock().expect("not poisoned").len(), 3, "the fourth level is already held");
    }

    /// Clause 4.22: an instance past its `expiresAt` is invalid, so the linked
    /// state carries it no longer.
    #[tokio::test]
    async fn an_expired_instance_of_a_linked_entity_is_not_read() {
        let held = cycle();
        let three_hops = "\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/next\"{\"https://example.com/zone\"}}}";
        let linked = gather(&held, three_hops).await;
        let c = linked.get(&"urn:c".parse().expect("valid URI")).expect("c is gathered");
        assert!(c.attributes().get("https://example.com/zone").is_none(), "the expired zone is left out");
    }
}
