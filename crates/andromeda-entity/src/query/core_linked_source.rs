//! Where the Core reads and both kinds of notification read their Linked
//! Entities from (clause 4.5.23).
//!
//! The traversal, its order and its bounds belong to the Linked Entity engine;
//! this source answers its two questions. The local store answers for what it
//! holds, each state compacted under the request context, and the remainder of
//! the distributed system answers through [`RemoteLinkedEntitySource`] for an
//! `objectType`-annotated identifier the store does not hold (clause
//! 4.5.23.1). A Core `join` is a spec parameter every conformant holder honours
//! (Tables 6.4.3.2-1 and 6.5.3.1-1), so a holder is always asked for the whole
//! remaining depth.

use crate::{error::EntityError, port::retrieved_entity::RetrievedEntity, query::EntityQueryService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::{
    candidate::LinkedCandidate,
    representation::LinkedRepresentation,
    source::{LevelRead, LinkedSource, RemoteReach, RemoteRead, RemoteResolution},
    typed::TypedEntities,
};
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, entity_system_times::EntitySystemTimes, notification::SysAttrsSnapshot};
use andromeda_ngsi_ld_representation::{linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use chrono::Utc;
use indexmap::IndexMap;
use std::{
    future::{Future, ready},
    num::NonZeroU32,
};

/// The Linked Entities of one Core read or notification: the branch they are
/// read from, whether they carry their clause 4.8 system times, and the
/// context they are compacted under.
#[derive(Clone, Copy)]
pub struct CoreLinkedSource<'s> {
    /// The service holding the store and the remote half.
    service: &'s EntityQueryService,
    /// The branch the entities are read from.
    target: &'s ContextTarget,
    /// Whether the Linked Entities carry their system times.
    sys_attrs: SysAttrsInclusion,
    /// The context they are compacted under.
    context: &'s ContextSpec,
    /// The Linked Entities the holders of the read's roots nested under the
    /// roots' Relationships, read into the model: they stand for an entity the
    /// store does not hold.
    carried: &'s [ReceivedState],
}

impl<'s> CoreLinkedSource<'s> {
    /// The source reading `target` through `service`, compacting under
    /// `context`.
    #[must_use]
    pub const fn new(
        service: &'s EntityQueryService,
        target: &'s ContextTarget,
        sys_attrs: SysAttrsInclusion,
        context: &'s ContextSpec,
    ) -> CoreLinkedSource<'s> {
        CoreLinkedSource {
            service,
            target,
            sys_attrs,
            context,
            carried: &[],
        }
    }

    /// The same source, answering for an entity the store does not hold with
    /// the one the roots' holders nested, if one did (clause 4.5.23).
    #[must_use]
    pub const fn with_carried(self, carried: &'s [ReceivedState]) -> CoreLinkedSource<'s> {
        CoreLinkedSource { carried, ..self }
    }

    /// Read the candidates the local store holds, in candidate order.
    ///
    /// A candidate the store does not hold, or whose state does not compact
    /// into an entity, is left out: clause 4.5.23 leaves such a Relationship a
    /// plain URI rather than failing the retrieval.
    async fn read_locally(&self, candidates: &[LinkedCandidate]) -> Result<LevelRead<Entity>, EntityError> {
        let ids: Vec<Identifier> = candidates.iter().map(|candidate| candidate.id.clone()).collect(); // clone: the store reads an owned id slice
        // Clause 4.8: the entity-level system times come back with each state
        // from the same read.
        let retrieved = self.service.reader.get_batch(self.target, &ids).await?;

        let compactor = self.service.engine().compactor(self.context);
        let entity_compactor = EntityCompactor::new(&compactor);
        let now = Utc::now();
        let mut nodes = IndexMap::with_capacity(retrieved.len());
        let mut edges = Vec::new();
        for candidate in candidates {
            let (state, times) = match retrieved.get(&candidate.id) {
                Some(RetrievedEntity { state, sys_attrs }) => (&**state, HeldTimes::Stored(sys_attrs)),
                None => match self.carried.iter().find(|carried| carried.state.id() == &candidate.id) {
                    Some(carried) => (&carried.state, HeldTimes::Carried(carried.system_times)),
                    None => continue,
                },
            };
            let read = EntityRead {
                id_override: Some(&candidate.id),
                sys_attrs: self.sys_attrs,
                now,
            };
            match entity_compactor.entity(state, &read).await {
                Ok(entity) => {
                    let entity = match (self.sys_attrs, times) {
                        (SysAttrsInclusion::Include, HeldTimes::Stored(times)) => entity.with_sys_attrs(times),
                        (SysAttrsInclusion::Include, HeldTimes::Carried(times)) => {
                            let mut entity = entity;
                            times.restore(&mut entity);
                            entity
                        }
                        (SysAttrsInclusion::Strip, HeldTimes::Stored(_) | HeldTimes::Carried(_)) => entity,
                    };
                    TypedEntities::edges(&entity, &mut edges);
                    nodes.insert(candidate.id.clone(), entity); // clone: the level keys its nodes by owned identifiers
                }
                Err(error) => {
                    tracing::debug!("Failed to compact linked entity {}: {}", candidate.id, error);
                }
            }
        }
        Ok(LevelRead { nodes, edges })
    }

    /// Ask the remainder of the system for one candidate.
    ///
    /// A source that cannot be consulted degrades the join rather than the
    /// request (clause 4.5.23): the candidate stays unresolved, and the read
    /// says what is missing.
    async fn fetch(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> RemoteRead<Entity> {
        let Some(remote) = self.service.remote_linked.as_ref() else {
            return RemoteRead::nothing();
        };
        match remote.fetch_linked(&candidate.id, depth, held, self.context).await {
            Ok(RemoteResolution { entities, mut failures }) => {
                for failure in &failures {
                    tracing::warn!("Linked entity {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
                }
                // A holder answers with the entity and, nested under its
                // Relationships, the Linked Entities it joined for the depth
                // that remains; every one of them is a node of the graph.
                let mut received = Vec::new();
                let mut pending = entities;
                while let Some(mut version) = pending.pop() {
                    pending.append(&mut version.linked);
                    received.push(version);
                }
                let compactor = self.service.engine().compactor(self.context);
                let entity_compactor = EntityCompactor::new(&compactor);
                let read = EntityRead {
                    id_override: None,
                    sys_attrs: self.sys_attrs,
                    now: Utc::now(),
                };
                let mut nodes = Vec::with_capacity(received.len());
                let mut edges = Vec::new();
                for version in received.into_iter().rev() {
                    match entity_compactor.entity(&version.state, &read).await {
                        Ok(mut entity) => {
                            if self.sys_attrs == SysAttrsInclusion::Include {
                                version.system_times.restore(&mut entity);
                            }
                            TypedEntities::edges(&entity, &mut edges);
                            nodes.push(entity);
                        }
                        // Clause 6.3.17: a payload the broker cannot read is
                        // an invalid answer, not a missing one.
                        Err(error) => failures.push(LinkedResolutionFailure::unusable(version.state.id(), &error)),
                    }
                }
                RemoteRead { nodes, edges, failures }
            }
            Err(error) => {
                tracing::warn!("Failed to resolve linked entity {} outside the local store: {}", candidate.id, error);
                RemoteRead::unreachable(&candidate.id, &error)
            }
        }
    }
}

/// Where a level's entity-level clause 4.8 system times come from.
#[derive(Clone, Copy)]
enum HeldTimes<'t> {
    /// The store's row.
    Stored(&'t SysAttrsSnapshot),
    /// What the holder that nested the entity wrote on it.
    Carried(EntitySystemTimes),
}

impl LinkedSource for CoreLinkedSource<'_> {
    type Representation = TypedEntities;
    type Error = EntityError;

    fn read_level(&self, candidates: &[LinkedCandidate]) -> impl Future<Output = Result<LevelRead<Entity>, EntityError>> + Send {
        self.read_locally(candidates)
    }

    fn fetch_remote(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> impl Future<Output = RemoteRead<Entity>> + Send {
        self.fetch(candidate, depth, held)
    }

    fn federates(&self) -> bool {
        self.target.permits_federation() && self.service.remote_linked.is_some()
    }

    fn remote_reach(&self, _candidate: &LinkedCandidate) -> impl Future<Output = RemoteReach> + Send {
        ready(RemoteReach::Subgraph)
    }
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
        port::{query::EntityQueryStore, reader::EntityReader},
        query::{core_linked_source::*, remote_linked::RemoteLinkedEntitySource},
        query_outcome::QueryOutcome,
        spatial_acceleration::SpatialAcceleration,
        storage_profile::StorageProfile,
    };
    use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_linked_retrieval::{
        graph::{LinkedGraph, RootEdges},
        join::TraversalBounds,
        traversal::traverse,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        filter::language_targeting::LanguageTargeting,
        geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
        page_window::PageWindow,
        projection::returned_attributes::ReturnedAttributes,
    };
    use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailureKind;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use chrono::DateTime;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex, MutexGuard},
    };

    /// Traverse `depth` levels from `roots` through the Core source, under
    /// [`context`], excluding `contained_by`.
    async fn resolve(service: &EntityQueryService, roots: &[Entity], depth: u32, contained_by: &[Identifier]) -> LinkedGraph<Entity> {
        let context = context();
        let target = ContextTarget::Live;
        let source = CoreLinkedSource::new(service, &target, SysAttrsInclusion::Strip, &context);
        let bounds = TraversalBounds::of(NonZeroU32::new(depth), Some(contained_by));
        traverse(&source, RootEdges::of::<TypedEntities>(roots), &bounds).await.expect("resolves")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    /// A root entity whose single Relationship names `targets` in order.
    fn root(id: &str, targets: &[&str]) -> Entity {
        let mut entity = json!({"id": id, "type": "Thing"});
        if !targets.is_empty() {
            entity["linksTo"] = json!({"type": "Relationship", "object": targets});
        }
        serde_json::from_value(entity).expect("entity")
    }

    /// A root entity whose single Relationship names `targets` and annotates
    /// them with the clause 4.5.23.1 `objectType` that declares each object an
    /// Internal Linked Entity.
    fn root_with_object_type(id: &str, targets: &[&str]) -> Entity {
        serde_json::from_value(json!({
            "id": id,
            "type": "Thing",
            "linksTo": {"type": "Relationship", "object": targets, "objectType": "Thing"}
        }))
        .expect("entity")
    }

    /// The state the store holds for an entity whose single Relationship names
    /// `targets` in order, or that has no attribute when `targets` is empty.
    /// The store holds states; the traversal compacts what it reads.
    fn stored(id: &str, targets: &[&str]) -> Arc<EntityState> {
        let mut document = json!({"@id": id, "@type": ["https://example.com/Thing"]});
        if !targets.is_empty() {
            let objects: Vec<Value> = targets.iter().map(|target| json!({"@id": target})).collect();
            document["https://example.com/linksTo"] = json!([{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": objects
            }]);
        }
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state"))
    }

    /// A store holding a fixed set of states, keyed by id.
    struct StubStore {
        entities: HashMap<Identifier, Arc<EntityState>>,
    }

    impl StubStore {
        fn new(entities: impl IntoIterator<Item = (Identifier, Arc<EntityState>)>) -> StubStore {
            StubStore {
                entities: entities.into_iter().collect(),
            }
        }
    }

    fn unsupported(method: &str) -> EntityError {
        panic!("StubStore::{method} is not exercised by the traversal harness")
    }

    #[async_trait]
    impl EntityReader for StubStore {
        async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
            Err(unsupported("get"))
        }

        async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
            Ok(ids
                .iter()
                .filter_map(|id| {
                    self.entities.get(id).map(|state| {
                        let retrieved = RetrievedEntity {
                            state: Arc::clone(state),
                            sys_attrs: SysAttrsSnapshot {
                                created_at: DateTime::<Utc>::UNIX_EPOCH,
                                modified_at: DateTime::<Utc>::UNIX_EPOCH,
                                expires_at: None,
                            },
                        };
                        (id.clone(), retrieved) // clone: the port hands back owned keys
                    })
                })
                .collect())
        }

        async fn get_with_sys_attrs(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _returned: &ReturnedAttributes<'_>,
        ) -> Result<Option<RetrievedEntity>, EntityError> {
            Err(unsupported("get_with_sys_attrs"))
        }

        async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
            Err(unsupported("get_snapshot"))
        }

        async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
            Err(unsupported("get_snapshot_batch"))
        }

        async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
            Err(unsupported("get_sys_attrs"))
        }

        async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
            Err(unsupported("get_sys_attrs_batch"))
        }
    }

    #[async_trait]
    impl EntityQueryStore for StubStore {
        fn profile(&self) -> &StorageProfile {
            panic!("the traversal never inspects the store's profile")
        }

        async fn query_compiled(
            &self,
            _target: &ContextTarget,
            _compiled: &CompiledEntityQuery,
            _window: &PageWindow,
            _count: CountPolicy,
        ) -> Result<QueryOutcome<RetrievedEntity>, EntityError> {
            Err(unsupported("query_compiled"))
        }

        async fn query_ids(
            &self,
            _target: &ContextTarget,
            _compiled: &CompiledEntityQuery,
            _window: &PageWindow,
        ) -> Result<QueryOutcome<Identifier>, EntityError> {
            Err(unsupported("query_ids"))
        }
    }

    /// One recorded call into the remote Linked Entity source.
    #[derive(Debug, PartialEq, Eq)]
    struct RemoteCall {
        id: Identifier,
        remaining_depth: NonZeroU32,
        visited: Vec<Identifier>,
    }

    /// What the stubbed remainder of the system answers with.
    #[derive(Debug)]
    enum RemoteOutcome {
        /// The entities each requested identifier resolves to.
        Returns(HashMap<Identifier, Vec<Entity>>),
        /// The source was reached but reported the entity as unresolvable.
        Reports(LinkedResolutionFailureKind),
        /// The remainder of the system cannot be consulted at all.
        Fails,
    }

    /// A [`RemoteLinkedEntitySource`] that records every call it receives.
    #[derive(Debug)]
    struct RecordingSource {
        calls: Mutex<Vec<RemoteCall>>,
        outcome: RemoteOutcome,
    }

    impl RecordingSource {
        fn new(outcome: RemoteOutcome) -> Arc<RecordingSource> {
            Arc::new(RecordingSource {
                calls: Mutex::new(Vec::new()),
                outcome,
            })
        }

        fn returning(entities: impl IntoIterator<Item = (Identifier, Vec<Entity>)>) -> Arc<RecordingSource> {
            RecordingSource::new(RemoteOutcome::Returns(entities.into_iter().collect()))
        }

        fn reporting(kind: LinkedResolutionFailureKind) -> Arc<RecordingSource> {
            RecordingSource::new(RemoteOutcome::Reports(kind))
        }

        fn failing() -> Arc<RecordingSource> {
            RecordingSource::new(RemoteOutcome::Fails)
        }

        fn calls(&self) -> MutexGuard<'_, Vec<RemoteCall>> {
            self.calls.lock().expect("the recording lock is never poisoned")
        }
    }

    #[async_trait]
    impl RemoteLinkedEntitySource for RecordingSource {
        async fn fetch_linked(
            &self,
            id: &Identifier,
            remaining_depth: NonZeroU32,
            visited: &[Identifier],
            context: &ContextSpec,
        ) -> Result<RemoteResolution<ReceivedState>, EntityError> {
            self.calls().push(RemoteCall {
                id: id.clone(), // clone: the recording owns the identifier it reports
                remaining_depth,
                visited: visited.to_vec(),
            });
            match &self.outcome {
                RemoteOutcome::Returns(entities) => {
                    // A holder's answer is read into the model under the
                    // context it is spelled in, as the forwarding reads it.
                    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
                    let expansion = EntityExpansion::prepare(&engine, context).await.expect("the context processes");
                    let mut received = Vec::new();
                    for entity in entities.get(id).cloned().unwrap_or_default() {
                        // clone: the source hands back owned entities
                        received.push(expansion.received(entity).await.expect("the answer reads"));
                    }
                    Ok(RemoteResolution {
                        entities: received,
                        failures: Vec::new(),
                    })
                }
                RemoteOutcome::Reports(kind) => Ok(RemoteResolution {
                    entities: Vec::new(),
                    failures: vec![LinkedResolutionFailure {
                        id: id.clone(), // clone: the failure owns the identifier it names
                        kind: *kind,
                        detail: "the context source answered with a refusal".to_owned(),
                    }],
                }),
                RemoteOutcome::Fails => Err(EntityError::from(PersistenceFailure::Unavailable)),
            }
        }
    }

    fn service(store: StubStore) -> EntityQueryService {
        build_service(store, None)
    }

    fn service_with_remote(store: StubStore, source: &Arc<RecordingSource>) -> EntityQueryService {
        build_service(store, Some(Arc::clone(source) as Arc<dyn RemoteLinkedEntitySource>)) // clone: the test keeps its handle on the recording
    }

    fn build_service(store: StubStore, remote_linked: Option<Arc<dyn RemoteLinkedEntitySource>>) -> EntityQueryService {
        let store = Arc::new(store);
        EntityQueryService::builder()
            .reader(store.clone() as Arc<dyn EntityReader>) // clone: the test double supplies both focused capabilities
            .query_store(store as Arc<dyn EntityQueryStore>)
            .engine(JsonLdEngine::builder().allow_remote(false).build().expect("build engine"))
            .spatial(SpatialAcceleration::StoreVerified)
            .tenant(TenantRef::UnnamedDefault)
            .remote_linked(remote_linked)
            .geo_admission(GeoQueryAdmission::new(
                geometry_census_limits(&RequestLimitsConfig::default()),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
            .build()
    }

    /// A context mapping the tests' one term to a deterministic,
    /// offline-resolvable IRI, so compaction needs no network fetch.
    fn context() -> ContextSpec {
        ContextSpec::from_body(JsonLiteral::admit(&json!({"linksTo": "https://example.com/linksTo"}).to_string()).expect("an admissible literal"))
    }

    fn level(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive join level")
    }

    fn ids(resolved: &LinkedGraph<Entity>) -> Vec<&str> {
        resolved.nodes.keys().map(Identifier::as_str).collect()
    }

    /// Clause 4.5.23: the flat representation appends linked entities in the
    /// order the traversal encounters them. A root naming C before B, whose B
    /// in turn names D, resolves breadth-first as `[C, B]` then `[D]`.
    #[tokio::test]
    async fn breadth_first_traversal_keeps_first_encounter_order() {
        let store = StubStore::new([
            (uri("urn:ngsi-ld:T:C"), stored("urn:ngsi-ld:T:C", &[])),
            (uri("urn:ngsi-ld:T:B"), stored("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:D"])),
            (uri("urn:ngsi-ld:T:D"), stored("urn:ngsi-ld:T:D", &[])),
        ]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service(store), &roots, 2, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D"]);
    }

    /// A cycle A -> B -> A terminates: the visited set excludes the root, so B
    /// is resolved once and its back-reference resolves nothing.
    #[tokio::test]
    async fn a_cycle_back_to_the_root_terminates() {
        let store = StubStore::new([(uri("urn:ngsi-ld:T:B"), stored("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:A"]))]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service(store), &roots, 5, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B"]);
    }

    /// Clause 4.5.23 `containedBy`: an entity the client already holds is not
    /// retrieved again, and is not descended into.
    #[tokio::test]
    async fn contained_by_excludes_the_identifiers_it_lists() {
        let store = StubStore::new([
            (uri("urn:ngsi-ld:T:B"), stored("urn:ngsi-ld:T:B", &[])),
            (uri("urn:ngsi-ld:T:C"), stored("urn:ngsi-ld:T:C", &[])),
        ]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"])];
        let contained_by = [uri("urn:ngsi-ld:T:B")];

        let resolved = resolve(&service(store), &roots, 1, &contained_by).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:C"]);
    }

    /// `joinLevel=1` follows exactly one hop: B is resolved, the D it links to
    /// is not.
    #[tokio::test]
    async fn join_level_one_stops_after_the_first_hop() {
        let store = StubStore::new([
            (uri("urn:ngsi-ld:T:B"), stored("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:D"])),
            (uri("urn:ngsi-ld:T:D"), stored("urn:ngsi-ld:T:D", &[])),
        ]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service(store), &roots, 1, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B"]);
    }

    /// Clause 4.5.23 admits any positive `joinLevel`; a six-entity chain
    /// requested at level 7 resolves all five non-root links.
    #[tokio::test]
    async fn a_join_level_beyond_the_deleted_cap_resolves_the_whole_chain() {
        let chain = [
            "urn:ngsi-ld:T:1",
            "urn:ngsi-ld:T:2",
            "urn:ngsi-ld:T:3",
            "urn:ngsi-ld:T:4",
            "urn:ngsi-ld:T:5",
            "urn:ngsi-ld:T:6",
        ];
        let store = StubStore::new(chain.iter().enumerate().map(|(index, id)| {
            let targets: Vec<&str> = chain.get(index + 1).map(|next| vec![*next]).unwrap_or_default();
            (uri(id), stored(id, &targets))
        }));
        let roots = vec![root(chain[0], &[chain[1]])];

        let resolved = resolve(&service(store), &roots, 7, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec![chain[1], chain[2], chain[3], chain[4], chain[5]]);
    }

    /// Clause 4.5.23.1: a Relationship annotated with an `objectType` declares
    /// its object an Internal Linked Entity, so an id the local store does not
    /// hold is asked of the remainder of the system, with the levels still to
    /// resolve, and with the already-walked identifiers so the remote side does
    /// not send back what the requester already holds.
    #[tokio::test]
    async fn an_absent_annotated_candidate_is_fetched_with_the_remaining_depth_and_the_visited_ids() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![root("urn:ngsi-ld:T:B", &[])])]);
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(StubStore::new([]), &source), &roots, 2, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B"]);
        pretty_assertions::assert_eq!(
            *source.calls(),
            vec![RemoteCall {
                id: uri("urn:ngsi-ld:T:B"),
                remaining_depth: level(2),
                visited: vec![uri("urn:ngsi-ld:T:A")],
            }]
        );
    }

    /// The flat append order of clause 6.4.3.2 is per level: whatever the local
    /// store answered for a level comes first, then what the remainder of the
    /// system answered for the same level, here the locally held `L`, named
    /// second, still precedes the remotely held `R`, named first.
    #[tokio::test]
    async fn remotely_resolved_entities_follow_the_local_ones_of_the_same_level() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:R"), vec![root("urn:ngsi-ld:T:R", &[])])]);
        let roots: Vec<Entity> = vec![
            serde_json::from_value(json!({
                "id": "urn:ngsi-ld:T:A",
                "type": "Thing",
                "linksTo": {"type": "Relationship", "object": "urn:ngsi-ld:T:R", "objectType": "Thing"},
                "alsoLinksTo": {"type": "Relationship", "object": "urn:ngsi-ld:T:L"}
            }))
            .expect("entity"),
        ];
        let store = StubStore::new([(uri("urn:ngsi-ld:T:L"), stored("urn:ngsi-ld:T:L", &[]))]);

        let resolved = resolve(&service_with_remote(store, &source), &roots, 1, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:L", "urn:ngsi-ld:T:R"]);
    }

    /// Clause 4.5.23.1 gates non-local resolution on the `objectType`
    /// annotation: an unannotated Relationship names a plain URI, which stays a
    /// plain URI rather than becoming a forwarded retrieval.
    #[tokio::test]
    async fn an_absent_unannotated_candidate_is_never_fetched_remotely() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![root("urn:ngsi-ld:T:B", &[])])]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(StubStore::new([]), &source), &roots, 1, &[]).await;

        assert!(resolved.nodes.is_empty());
        assert!(resolved.failures.is_empty(), "resolution reported failures: {:?}", resolved.failures);
        assert!(source.calls().is_empty());
    }

    /// A remainder of the system that cannot be consulted degrades the join
    /// rather than the request: the retrieval still succeeds and the
    /// unresolvable id is simply absent (clause 4.5.23).
    #[tokio::test]
    async fn a_failing_remote_source_leaves_the_identifier_unresolved() {
        let source = RecordingSource::failing();
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(StubStore::new([]), &source), &roots, 1, &[]).await;

        assert!(resolved.nodes.is_empty());
        pretty_assertions::assert_eq!(source.calls().len(), 1);
        pretty_assertions::assert_eq!(
            resolved.failures,
            vec![LinkedResolutionFailure {
                id: uri("urn:ngsi-ld:T:B"),
                kind: LinkedResolutionFailureKind::Unreachable,
                detail: EntityError::from(PersistenceFailure::Unavailable).to_string(),
            }]
        );
    }

    /// A source that answers but cannot produce the Linked Entity reports why,
    /// and the traversal carries that report back verbatim: the caller is the
    /// one that knows whether it becomes a clause 6.3.17 response warning or a
    /// log line.
    #[tokio::test]
    async fn a_reported_resolution_failure_travels_back_to_the_caller() {
        let source = RecordingSource::reporting(LinkedResolutionFailureKind::Refused);
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(StubStore::new([]), &source), &roots, 1, &[]).await;

        assert!(resolved.nodes.is_empty());
        pretty_assertions::assert_eq!(
            resolved.failures,
            vec![LinkedResolutionFailure {
                id: uri("urn:ngsi-ld:T:B"),
                kind: LinkedResolutionFailureKind::Refused,
                detail: "the context source answered with a refusal".to_owned(),
            }]
        );
    }

    /// A remotely resolved entity is a root of the next level like any other:
    /// its Relationships are followed, including back into the local store.
    #[tokio::test]
    async fn a_remotely_resolved_entity_is_traversed_as_a_next_level_root() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"])])]);
        let store = StubStore::new([(uri("urn:ngsi-ld:T:C"), stored("urn:ngsi-ld:T:C", &[]))]);
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(store, &source), &roots, 2, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    }

    /// The sub-graph a remote broker returns around a fetched candidate is
    /// already resolved, with the depth budget that broker was given. Those
    /// extra entities join the result and the visited guard, but they are not
    /// walked again: `C` arrives as `B`'s link at the second and last level, so
    /// the `D` it names is out of range, and `C` itself is never fetched.
    #[tokio::test]
    async fn a_remote_brokers_extra_entities_are_neither_re_fetched_nor_expanded() {
        let source = RecordingSource::returning([(
            uri("urn:ngsi-ld:T:B"),
            vec![
                root_with_object_type("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]),
                root_with_object_type("urn:ngsi-ld:T:C", &["urn:ngsi-ld:T:D"]),
            ],
        )]);
        let store = StubStore::new([(uri("urn:ngsi-ld:T:D"), stored("urn:ngsi-ld:T:D", &[]))]);
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = resolve(&service_with_remote(store, &source), &roots, 2, &[]).await;

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
        pretty_assertions::assert_eq!(source.calls().iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
    }
}
