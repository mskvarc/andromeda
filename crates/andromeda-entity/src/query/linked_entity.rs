//! Resolving the entities a result set links to (clause 4.5.23 `join`).
//!
//! The walk is breadth-first: the entities the roots name are resolved first,
//! then the entities those name, one level at a time. Each level is resolved in
//! two halves — the local store answers for what it holds, and the remainder of
//! the distributed system answers for the `objectType`-annotated ids it does
//! not (clause 4.5.23.1).
//!
//! Clause 6.4.3.2 requires `join=flat` to put the linked entities alongside the
//! roots; it does not dictate the order they arrive in. Andromeda guarantees
//! one anyway, because an unordered result would make every response body
//! irreproducible: the traversal's first-encounter order is the append order,
//! and within a level the locally held entities precede the ones fetched from
//! elsewhere. [`EntityQueryService::resolve_linked_entities`] states the
//! contract precisely.
//!
//! There is no depth cap. Clause 4.5.23 admits any positive `joinLevel`, and
//! termination is guaranteed by the visited set instead: every candidate is
//! resolved at most once, so the work is bounded by the number of reachable
//! entities rather than by the client's number.

use crate::{
    error::EntityError,
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_model::{attribute::instance::Attribute, entity::Entity};
use andromeda_ngsi_ld_representation::{
    linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind},
    sys_attrs::SysAttrsInclusion,
};
use indexmap::{IndexMap, IndexSet};
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
};

/// One traversal candidate: the id a Relationship names, together with the
/// `objectType` annotations its referencing Relationships carried.
///
/// A non-empty `object_types` is the clause 4.5.23.1 eligibility gate for
/// resolving the id somewhere other than the local store: only a Relationship
/// annotated with an `objectType` declares its object to be an Internal Linked
/// Entity that a registered context source may hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedCandidate {
    /// The linked entity's identifier.
    pub id: Uri,
    /// The entity types every referencing Relationship annotated the object
    /// with, in first-encounter order. Empty when no referencing Relationship
    /// carried an `objectType`.
    pub object_types: Vec<NgsiLdReference>,
}

/// Everything a join traversal produced: the entities it resolved, and the
/// Linked Entities it could not.
///
/// The two travel together because a caller that renders the first has to
/// account for the second — an HTTP binding turns each failure into a clause
/// 6.3.17 response warning, a notification logs it — and neither of them is an
/// error that should fail the operation.
#[derive(Debug, Default)]
pub struct ResolvedLinkedEntities {
    /// The linked entities, keyed by identifier in the traversal's append
    /// order.
    pub entities: IndexMap<Uri, Entity>,
    /// Every Linked Entity the traversal left unresolved, in the order the
    /// attempts were made.
    pub failures: Vec<LinkedResolutionFailure>,
}

/// One level of the traversal, as resolved.
struct ResolvedLevel {
    /// The level's entities in append order: the locally held ones in
    /// candidate order, then the ones fetched from elsewhere.
    entities: IndexMap<Uri, Entity>,
    /// What the level could not resolve.
    failures: Vec<LinkedResolutionFailure>,
}

impl EntityQueryService {
    /// Resolve the entities `entities` link to, up to `join_level` hops away.
    ///
    /// The result is keyed by identifier in breadth-first encounter order and
    /// never contains a root, an id listed in `contained_by` (clause 4.5.23
    /// excludes the entities the client already holds), or the same id twice.
    /// An id that nothing backs is simply absent — an unresolvable Relationship
    /// leaves the plain URI in place rather than failing the request — and,
    /// when something was reachable but unhelpful, it is named in
    /// [`ResolvedLinkedEntities::failures`] so the caller can say so.
    ///
    /// # Ordering
    ///
    /// Clause 6.4.3.2 requires the linked entities to accompany the roots but
    /// does not fix their order. Andromeda fixes it, so that the same request
    /// against the same data always renders the same body: the map's
    /// first-encounter order *is* the append order, and within one level the
    /// local store's answers come first, in candidate order, followed by the
    /// answers from the remainder of the system (clause 4.5.23.1), in the order
    /// their candidates were encountered. Non-local resolution is folded in
    /// inside the level it belongs to, never appended after the traversal.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the store cannot be read. A single linked
    /// entity that fails to compact, and a remote source that cannot answer,
    /// are reported rather than propagated.
    pub async fn resolve_linked_entities(
        &self,
        target: &ContextTarget,
        entities: &[Entity],
        join_level: NonZeroU32,
        sys_attrs: SysAttrsInclusion,
        context: &ContextSpec,
        contained_by: &HashSet<Uri>,
    ) -> Result<ResolvedLinkedEntities, EntityError> {
        let mut resolved = ResolvedLinkedEntities::default();
        // The roots and the client-held ids are excluded from the first level
        // and from every level after it, so a cycle back into them terminates.
        // The guard keeps insertion order because it doubles as the list of
        // already-held ids a non-local fetch hands to the remote broker.
        let mut visited: IndexSet<Uri> = contained_by.iter().cloned().collect(); // clone: the guard owns the ids it excludes
        visited.extend(entities.iter().map(|entity| entity.id.clone())); // clone: the guard outlives this borrow of the roots

        let mut candidates = collect_candidates(entities.iter(), &visited);
        let mut remaining = Some(join_level);

        while let Some(depth) = remaining
            && !candidates.is_empty()
        {
            visited.extend(candidates.iter().map(|candidate| candidate.id.clone())); // clone: the guard outlives this level's candidates
            let level = self.resolve_level(target, &candidates, depth, &visited, sys_attrs, context).await?;
            // Everything the level produced — including the sub-graph a remote
            // broker returned around a candidate — is now held, so no later
            // level asks for it again.
            visited.extend(level.entities.keys().cloned()); // clone: the guard outlives this level's entities
            remaining = NonZeroU32::new(depth.get() - 1);
            // The next level's candidates are read before this level is folded
            // into the result, so the result's insertion order stays exactly
            // the order the levels were resolved in.
            let next = match remaining {
                Some(_) => collect_candidates(expandable(&candidates, &level), &visited),
                None => Vec::new(),
            };
            candidates = next;
            resolved.entities.extend(level.entities);
            resolved.failures.extend(level.failures);
        }

        Ok(resolved)
    }

    /// Resolve one level from the local store and, for what the store does not
    /// hold, from the remainder of the system.
    ///
    /// Clause 4.5.23.1 gates the second half: only a candidate whose
    /// referencing Relationship carried an `objectType` is an Internal Linked
    /// Entity that some other context source may hold. A candidate without one
    /// that the store does not hold stays a plain URI.
    ///
    /// `depth` is the number of levels still to resolve, this one included.
    async fn resolve_level(
        &self,
        target: &ContextTarget,
        candidates: &[LinkedCandidate],
        depth: NonZeroU32,
        visited: &IndexSet<Uri>,
        sys_attrs: SysAttrsInclusion,
        context: &ContextSpec,
    ) -> Result<ResolvedLevel, EntityError> {
        let entities = self.resolve_level_locally(target, candidates, sys_attrs, context).await?;
        let mut level = ResolvedLevel {
            entities,
            failures: Vec::new(),
        };
        let Some(source) = target.permits_federation().then_some(self.remote_linked.as_ref()).flatten() else {
            return Ok(level);
        };
        for candidate in candidates {
            if candidate.object_types.is_empty() || level.entities.contains_key(&candidate.id) {
                continue;
            }
            fetch_remote_candidate(source.as_ref(), candidate, depth, visited, context, &mut level).await;
        }
        Ok(level)
    }

    /// Read one level's candidates from the local store, in candidate order.
    ///
    /// A candidate the store does not hold, or whose payload does not compact
    /// into an entity, is dropped from the level: clause 4.5.23 leaves such a
    /// Relationship as a plain URI rather than failing the whole retrieval.
    async fn resolve_level_locally(
        &self,
        target: &ContextTarget,
        candidates: &[LinkedCandidate],
        sys_attrs: SysAttrsInclusion,
        context: &ContextSpec,
    ) -> Result<IndexMap<Uri, Entity>, EntityError> {
        let ids: Vec<Uri> = candidates.iter().map(|candidate| candidate.id.clone()).collect(); // clone: the store reads an owned id slice
        let mut payloads = self.reader.get_batch(target, &ids).await?;
        let sys_attr_snapshots = match sys_attrs {
            SysAttrsInclusion::Include => self.reader.get_sys_attrs_batch(target, &ids).await?,
            SysAttrsInclusion::Strip => HashMap::new(),
        };

        let compactor = self.engine().compactor(context);
        let mut level = IndexMap::with_capacity(payloads.len());
        for candidate in candidates {
            let Some(expanded) = payloads.remove(&candidate.id) else {
                continue;
            };
            match self.compact_to_entity(expanded, &compactor, Some(candidate.id.as_str()), sys_attrs).await {
                Ok(mut entity) => {
                    if let Some(snapshot) = sys_attr_snapshots.get(&candidate.id) {
                        entity = entity.with_sys_attrs(snapshot);
                    }
                    level.insert(candidate.id.clone(), entity); // clone: the result map owns its keys
                }
                Err(error) => {
                    tracing::debug!("Failed to compact linked entity {}: {}", candidate.id, error);
                }
            }
        }
        Ok(level)
    }
}

/// The level's entities whose own Relationships the next level walks.
///
/// A candidate this level resolved — locally, or by being fetched from a remote
/// broker — is a next-level root like any other. The extra entities that came
/// back around a fetched candidate are not: they are the remote broker's
/// already-resolved sub-tree, walked with the depth budget it was given, and
/// walking them again here would spend levels twice and reach further than the
/// client's `joinLevel` allows.
fn expandable<'a>(candidates: &'a [LinkedCandidate], level: &'a ResolvedLevel) -> impl Iterator<Item = &'a Entity> {
    candidates.iter().filter_map(|candidate| level.entities.get(&candidate.id))
}

/// Ask the remainder of the system for one candidate and fold what it answers
/// into the level it belongs to.
///
/// A source that cannot answer degrades the join rather than the request
/// (clause 4.5.23): the candidate stays unresolved, leaving the Relationship
/// that named it a plain URI, and the level carries a description of what is
/// missing back to the caller.
async fn fetch_remote_candidate(
    source: &dyn RemoteLinkedEntitySource,
    candidate: &LinkedCandidate,
    depth: NonZeroU32,
    visited: &IndexSet<Uri>,
    context: &ContextSpec,
    level: &mut ResolvedLevel,
) {
    // The candidate itself is already in the guard; the remainder of the system
    // is told about everything except the entity it is being asked for.
    let held: Vec<Uri> = visited.iter().filter(|id| **id != candidate.id).cloned().collect(); // clone: the port takes the ids as an owned slice
    match source.fetch_linked(&candidate.id, depth, &held, context).await {
        Ok(fetched) => {
            for entity in fetched.entities {
                // The remote broker resolved the deeper levels in the same
                // round trip. Whatever it returns that the requester already
                // holds is dropped: clause 4.5.23 `containedBy` keeps those out
                // of the result.
                if entity.id != candidate.id && visited.contains(&entity.id) {
                    continue;
                }
                level.entities.entry(entity.id.clone()).or_insert(entity); // clone: the level map owns its keys
            }
            for failure in &fetched.failures {
                tracing::warn!("Linked entity {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
            }
            level.failures.extend(fetched.failures);
        }
        Err(error) => {
            tracing::warn!("Failed to resolve linked entity {} outside the local store: {}", candidate.id, error);
            level.failures.push(LinkedResolutionFailure {
                id: candidate.id.clone(), // clone: the failure outlives this level's candidates
                kind: LinkedResolutionFailureKind::Unreachable,
                detail: error.to_string(),
            });
        }
    }
}

/// Collect, in first-encounter order, every not-yet-visited identifier the
/// given entities' Relationships and `ListRelationships` name.
///
/// `objectList` order is preserved, and an id named by several Relationships
/// appears once, carrying the union of their `objectType` annotations.
fn collect_candidates<'a>(entities: impl IntoIterator<Item = &'a Entity>, visited: &IndexSet<Uri>) -> Vec<LinkedCandidate> {
    let mut seen: IndexSet<Uri> = IndexSet::new();
    let mut candidates: Vec<LinkedCandidate> = Vec::new();
    for entity in entities {
        for value in entity.attrs.values() {
            for attr in value.instances() {
                collect_from_attribute(attr, visited, &mut seen, &mut candidates);
            }
        }
    }
    candidates
}

/// Record the identifiers one attribute instance points at.
///
/// `seen` and `candidates` are index-aligned: `seen`'s position of an id is
/// the position of its [`LinkedCandidate`], which is how a repeated id folds
/// its `objectType` annotation into the entry already recorded for it.
fn collect_from_attribute(attr: &Attribute, visited: &IndexSet<Uri>, seen: &mut IndexSet<Uri>, candidates: &mut Vec<LinkedCandidate>) {
    let (objects, object_types): (Vec<&Uri>, Vec<NgsiLdReference>) = match attr {
        Attribute::Relationship(relationship) => (
            relationship.object.iter().collect(),
            relationship
                .object_type
                .iter()
                .flat_map(|types| types.iter())
                .cloned() // clone: the candidate owns the annotations it records
                .collect(),
        ),
        Attribute::ListRelationship(list) => (
            list.object_list.iter().collect(),
            list.object_type
                .iter()
                .flat_map(|types| types.iter())
                .cloned() // clone: the candidate owns the annotations it records
                .collect(),
        ),
        Attribute::Property(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::VocabProperty(_) => return,
    };

    for object in objects {
        if visited.contains(object) {
            continue;
        }
        if let Some(index) = seen.get_index_of(object) {
            if let Some(candidate) = candidates.get_mut(index) {
                for object_type in &object_types {
                    if !candidate.object_types.contains(object_type) {
                        candidate.object_types.push(object_type.clone()); // clone: the candidate owns the annotations it records
                    }
                }
            }
        } else {
            seen.insert(object.clone()); // clone: the duplicate guard owns the ids it records
            candidates.push(LinkedCandidate {
                id: object.clone(),                 // clone: the candidate owns its identifier
                object_types: object_types.clone(), // clone: each candidate owns its own annotation list
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entity_snapshot::EntitySnapshot,
        port::{query::EntityQueryStore, reader::EntityReader},
        query::remote_linked::RemoteLinkedEntities,
        query_outcome::QueryOutcome,
        storage_profile::StorageProfile,
    };
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
    use andromeda_spatial_index::entity::index::SpatialEntityIndex;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex, MutexGuard};

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    /// A root entity whose single Relationship names `targets` in order.
    fn root(id: &str, targets: &[&str]) -> Entity {
        serde_json::from_value(json!({
            "id": id,
            "type": "Thing",
            "linksTo": {"type": "Relationship", "object": targets}
        }))
        .expect("entity")
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

    /// The expanded NGSI-LD payload the store holds for an entity whose single
    /// Relationship names `targets` in order. The store speaks expanded
    /// JSON-LD; the traversal compacts what it reads.
    fn stored(id: &str, targets: &[&str]) -> Value {
        let objects: Vec<Value> = targets.iter().map(|target| json!({"@id": target})).collect();
        json!({
            "@id": id,
            "@type": ["https://example.com/Thing"],
            "https://example.com/linksTo": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": objects
            }]
        })
    }

    /// A store holding a fixed set of expanded payloads, keyed by id.
    struct StubStore {
        entities: HashMap<Uri, Value>,
    }

    impl StubStore {
        fn new(entities: impl IntoIterator<Item = (Uri, Value)>) -> StubStore {
            StubStore {
                entities: entities.into_iter().collect(),
            }
        }
    }

    fn unsupported(method: &str) -> EntityError {
        EntityError::internal(format!("StubStore::{method} is not exercised by the traversal harness"))
    }

    #[async_trait]
    impl EntityReader for StubStore {
        async fn get(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<Value>, EntityError> {
            Err(unsupported("get"))
        }

        async fn get_batch(&self, _target: &ContextTarget, ids: &[Uri]) -> Result<HashMap<Uri, Value>, EntityError> {
            Ok(ids
                .iter()
                .filter_map(|id| self.entities.get(id).map(|value| (id.clone(), value.clone()))) // clone: the port hands back owned payloads
                .collect())
        }

        async fn get_snapshot(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<EntitySnapshot>, EntityError> {
            Err(unsupported("get_snapshot"))
        }

        async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, EntityError> {
            Err(unsupported("get_snapshot_batch"))
        }

        async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<SysAttrsSnapshot>, EntityError> {
            Err(unsupported("get_sys_attrs"))
        }

        async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, EntityError> {
            Err(unsupported("get_sys_attrs_batch"))
        }
    }

    #[async_trait]
    impl EntityQueryStore for StubStore {
        fn profile(&self) -> &StorageProfile {
            panic!("the traversal never inspects the store's profile")
        }

        async fn query_compiled(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, EntityError> {
            Err(unsupported("query_compiled"))
        }

        async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, EntityError> {
            Err(unsupported("query_ids"))
        }
    }

    /// One recorded call into the remote Linked Entity source.
    #[derive(Debug, PartialEq, Eq)]
    struct RemoteCall {
        id: Uri,
        remaining_depth: NonZeroU32,
        visited: Vec<Uri>,
    }

    /// What the stubbed remainder of the system answers with.
    #[derive(Debug)]
    enum RemoteOutcome {
        /// The entities each requested identifier resolves to.
        Returns(HashMap<Uri, Vec<Entity>>),
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

        fn returning(entities: impl IntoIterator<Item = (Uri, Vec<Entity>)>) -> Arc<RecordingSource> {
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
            id: &Uri,
            remaining_depth: NonZeroU32,
            visited: &[Uri],
            _context: &ContextSpec,
        ) -> Result<RemoteLinkedEntities, EntityError> {
            self.calls().push(RemoteCall {
                id: id.clone(), // clone: the recording owns the identifier it reports
                remaining_depth,
                visited: visited.to_vec(),
            });
            match &self.outcome {
                RemoteOutcome::Returns(entities) => Ok(RemoteLinkedEntities {
                    entities: entities.get(id).cloned().unwrap_or_default(), // clone: the source hands back owned entities
                    failures: Vec::new(),
                }),
                RemoteOutcome::Reports(kind) => Ok(RemoteLinkedEntities {
                    entities: Vec::new(),
                    failures: vec![LinkedResolutionFailure {
                        id: id.clone(), // clone: the failure owns the identifier it names
                        kind: *kind,
                        detail: "the context source answered with a refusal".to_owned(),
                    }],
                }),
                RemoteOutcome::Fails => Err(EntityError::internal("the remainder of the system is unreachable")),
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
        EntityQueryService::new(
            store.clone() as Arc<dyn EntityReader>, // clone: the test double supplies both focused capabilities
            store as Arc<dyn EntityQueryStore>,
            JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
            Arc::new(SpatialEntityIndex::new()),
            TenantRef::UnnamedDefault,
            remote_linked,
        )
    }

    /// A context mapping the tests' one term to a deterministic,
    /// offline-resolvable IRI, so compaction needs no network fetch.
    fn context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({"linksTo": "https://example.com/linksTo"})))
    }

    fn level(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive join level")
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    fn ids(resolved: &ResolvedLinkedEntities) -> Vec<&str> {
        resolved.entities.keys().map(Uri::as_str).collect()
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

        let resolved = service(store)
            .resolve_linked_entities(&live_target(), &roots, level(2), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D"]);
    }

    /// A cycle A -> B -> A terminates: the visited set excludes the root, so B
    /// is resolved once and its back-reference resolves nothing.
    #[tokio::test]
    async fn a_cycle_back_to_the_root_terminates() {
        let store = StubStore::new([(uri("urn:ngsi-ld:T:B"), stored("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:A"]))]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = service(store)
            .resolve_linked_entities(&live_target(), &roots, level(5), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

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
        let contained_by: HashSet<Uri> = [uri("urn:ngsi-ld:T:B")].into_iter().collect();

        let resolved = service(store)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &contained_by)
            .await
            .expect("resolves");

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

        let resolved = service(store)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B"]);
    }

    /// Clause 4.5.23 admits any positive `joinLevel`; a six-entity chain
    /// requested at level 7 resolves all five non-root links, which the
    /// deleted five-level cap would have truncated.
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

        let resolved = service(store)
            .resolve_linked_entities(&live_target(), &roots, level(7), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(ids(&resolved), vec![chain[1], chain[2], chain[3], chain[4], chain[5]]);
    }

    /// Clause 4.5.23.1: a Relationship annotated with an `objectType` declares
    /// its object an Internal Linked Entity, so an id the local store does not
    /// hold is asked of the remainder of the system — with the levels still to
    /// resolve, and with the already-walked identifiers so the remote side does
    /// not send back what the requester already holds.
    #[tokio::test]
    async fn an_absent_annotated_candidate_is_fetched_with_the_remaining_depth_and_the_visited_ids() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![root("urn:ngsi-ld:T:B", &[])])]);
        let roots = vec![root_with_object_type("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = service_with_remote(StubStore::new([]), &source)
            .resolve_linked_entities(&live_target(), &roots, level(2), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

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
    /// system answered for the same level — here the locally held `L`, named
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

        let resolved = service_with_remote(store, &source)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:L", "urn:ngsi-ld:T:R"]);
    }

    /// Clause 4.5.23.1 gates non-local resolution on the `objectType`
    /// annotation: an unannotated Relationship names a plain URI, which stays a
    /// plain URI rather than becoming a forwarded retrieval.
    #[tokio::test]
    async fn an_absent_unannotated_candidate_is_never_fetched_remotely() {
        let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![root("urn:ngsi-ld:T:B", &[])])]);
        let roots = vec![root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

        let resolved = service_with_remote(StubStore::new([]), &source)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        assert!(resolved.entities.is_empty());
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

        let resolved = service_with_remote(StubStore::new([]), &source)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves despite the unreachable remainder");

        assert!(resolved.entities.is_empty());
        pretty_assertions::assert_eq!(source.calls().len(), 1);
        pretty_assertions::assert_eq!(
            resolved.failures,
            vec![LinkedResolutionFailure {
                id: uri("urn:ngsi-ld:T:B"),
                kind: LinkedResolutionFailureKind::Unreachable,
                detail: EntityError::internal("the remainder of the system is unreachable").to_string(),
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

        let resolved = service_with_remote(StubStore::new([]), &source)
            .resolve_linked_entities(&live_target(), &roots, level(1), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        assert!(resolved.entities.is_empty());
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

        let resolved = service_with_remote(store, &source)
            .resolve_linked_entities(&live_target(), &roots, level(2), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

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

        let resolved = service_with_remote(store, &source)
            .resolve_linked_entities(&live_target(), &roots, level(2), SysAttrsInclusion::Strip, &context(), &HashSet::new())
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(ids(&resolved), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
        pretty_assertions::assert_eq!(source.calls().iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
    }
}
