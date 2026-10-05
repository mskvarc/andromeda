//! The temporal join through the Linked Entity engine: what the temporal
//! source reads, what it asks the remainder of the system, and how the
//! request's list narrows what it resolved.

use crate::{
    error::TemporalError,
    instance_limits::TemporalInstanceLimits,
    linked::TemporalLinkedJoin,
    observed_instance::ObservedInstance,
    outcome::{TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        mutation_barrier::TemporalMutationBarrier,
        mutator::{TemporalDeleteScope, TemporalMutator},
        service_ports::TemporalServicePorts,
    },
    remote_linked::{RemoteTemporalLinkedEntities, RemoteTemporalLinkedSource, TemporalLinkedWindow},
    service::TemporalService,
    service_limits::TemporalServiceLimits,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    config::EntityConfig,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{reader::EntityReader, retrieved_entity::RetrievedEntity, retry_delay::RetryDelay},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_linked_retrieval::{
    admission::TemporalJoin,
    edge::{LinkedEdge, RemoteEligibility},
    plan::JoinPlan,
    source::RemoteReach,
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    temporal_format::TemporalFormat,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::{
    notification::SysAttrsSnapshot,
    temporal::{TemporalAggregationRow, TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    projection::grammar::ProjectionList,
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, history_delete::TemporalHistoryDeleteScope},
};
use andromeda_ngsi_ld_render::temporal::linked_projection::narrowed_temporal_entity;
use andromeda_ngsi_ld_representation::{
    join_mode::JoinMode,
    linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind},
};
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    iter,
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

fn uri(value: &str) -> Identifier {
    value.parse().expect("valid URI")
}

fn live_target() -> ContextTarget {
    ContextTarget::Live
}

/// A broker whose history holds, for each entity, one Relationship observation
/// per identifier that entity links to.
///
/// The graph is spelled as "this entity links to those", which is all the
/// traversal reads: how far it walks and what it refuses to walk twice.
struct StubBroker {
    /// Each entity's outgoing links, in declaration order.
    links: HashMap<Identifier, Vec<Identifier>>,
}

impl StubBroker {
    fn new(links: impl IntoIterator<Item = (Identifier, Vec<Identifier>)>) -> StubBroker {
        StubBroker {
            links: links.into_iter().collect(),
        }
    }

    /// The history rows one entity's links render into.
    ///
    /// Every known entity also carries one plain observation, so that a leaf
    /// with no outgoing link still has history in the window: an entity with an
    /// empty history is legitimately dropped from a join, and the traversal
    /// tests are about which entities are *reached*, not about that rule.
    fn rows(&self, entity_id: &Identifier) -> Vec<TemporalHistoryRow> {
        let instant = "2024-01-01T00:00:00Z".parse::<DateTime<Utc>>().expect("valid instant");
        let Some(targets) = self.links.get(entity_id) else {
            return Vec::new();
        };
        let reading = TemporalHistoryRow {
            attribute_name: "https://example.com/reading".parse::<AttributeName>().expect("valid attribute IRI"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")),
            observed_at: Some(instant),
            recorded_at: instant,
            created_at: Some(instant),
            modified_at: Some(instant),
            instance: InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:reading"))),
            metadata: None,
            partition_total: None,
        };
        iter::once(reading)
            .chain(targets.iter().map(|target| TemporalHistoryRow {
                attribute_name: "https://example.com/linksTo".parse::<AttributeName>().expect("valid attribute IRI"),
                attribute_type: AttributeType::Relationship,
                dataset_id: None,
                value: TemporalValue::Uri(target.as_uri().clone()), // clone: the row owns the object it names
                observed_at: Some(instant),
                recorded_at: instant,
                created_at: Some(instant),
                modified_at: Some(instant),
                instance: InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:{target}"))),
                metadata: None,
                partition_total: None,
            }))
            .collect()
    }
}

fn unsupported(method: &str) -> TemporalError {
    panic!("StubBroker::{method} is not exercised by the traversal harness")
}

#[async_trait]
impl TemporalHistoryReader for StubBroker {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::OnTheFly,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn entity_exists(&self, _target: &ContextTarget, entity_id: &Identifier) -> Result<bool, TemporalError> {
        Ok(self.links.contains_key(entity_id))
    }

    async fn entity_types(&self, _target: &ContextTarget, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        Ok(entity_ids
            .iter()
            .filter(|entity_id| self.links.contains_key(*entity_id))
            .map(|entity_id| (entity_id.clone(), Vec::new())) // clone: the index owns its validated entity URI.
            .collect())
    }

    async fn query_entity_ids(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledTemporalQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        Ok(TemporalEntityIdOutcome {
            candidates: Vec::new(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        })
    }

    async fn filter_leaf_truths(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        Err(unsupported("filter_leaf_truths"))
    }

    async fn query_entity_history(
        &self,
        _target: &ContextTarget,
        entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        Ok(TemporalHistoryOutcome {
            observations: self
                .rows(entity_id)
                .into_iter()
                .map(|row| TemporalHistoryObservation { row, identity: None })
                .collect(),
        })
    }

    async fn query_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(unsupported("query_entity_aggregation"))
    }
    async fn query_merged_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _entity_types: &[EntityTypeName],
        _compiled: &CompiledTemporalQuery,
        _answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(unsupported("query_merged_entity_aggregation"))
    }

    async fn recorded_attribute_shapes(
        &self,
        _target: &ContextTarget,
        _attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        Ok(HashMap::new())
    }
}

/// The current state of one known entity: a `Thing` holding no attribute.
fn thing(id: &Identifier) -> Arc<EntityState> {
    let thing = "https://example.com/Thing".parse::<EntityTypeName>().expect("valid type IRI");
    Arc::new(EntityState::new(id.clone(), EntityTypes::one(thing))) // clone: the state owns its identifier
}

#[async_trait]
impl EntityReader for StubBroker {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(self.links.contains_key(id).then(|| thing(id)))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(ids
            .iter()
            .filter(|id| self.links.contains_key(*id))
            .map(|id| {
                let retrieved = RetrievedEntity {
                    state: thing(id),
                    sys_attrs: SysAttrsSnapshot {
                        created_at: DateTime::<Utc>::UNIX_EPOCH,
                        modified_at: DateTime::<Utc>::UNIX_EPOCH,
                        expires_at: None,
                    },
                };
                (id.clone(), retrieved) // clone: the port hands back owned keys
            })
            .collect())
    }

    async fn get_with_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<RetrievedEntity>, EntityError> {
        panic!("get_with_sys_attrs is not exercised by the traversal harness")
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        panic!("get_snapshot is not exercised by the traversal harness")
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        panic!("get_snapshot_batch is not exercised by the traversal harness")
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        panic!("get_sys_attrs is not exercised by the traversal harness")
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        panic!("get_sys_attrs_batch is not exercised by the traversal harness")
    }
}

#[async_trait]
impl TemporalMutationBarrier for StubBroker {
    async fn synchronize(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<(), TemporalError> {
        Err(unsupported("synchronize"))
    }
}

#[async_trait]
impl TemporalMutator for StubBroker {
    async fn entity_exists(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<bool, TemporalError> {
        Err(unsupported("entity_exists"))
    }

    async fn attribute_exists(&self, _target: &ContextTarget, _entity_id: &Identifier, _attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        Err(unsupported("attribute_exists"))
    }

    async fn attribute_names_for_entity(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<Vec<AttributeName>, TemporalError> {
        Err(unsupported("attribute_names_for_entity"))
    }

    async fn delete_all_for_entity(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<u64, TemporalError> {
        Err(unsupported("delete_all_for_entity"))
    }

    async fn delete_attribute(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
        _dataset_id: Option<&Identifier>,
        _scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("delete_attribute"))
    }

    async fn instance_exists(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
    ) -> Result<bool, TemporalError> {
        Err(unsupported("instance_exists"))
    }

    async fn update_instance(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
        _replacement: &ObservedInstance,
    ) -> Result<(), TemporalError> {
        Err(unsupported("update_instance"))
    }

    async fn delete_instance(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
    ) -> Result<(), TemporalError> {
        Err(unsupported("delete_instance"))
    }

    async fn count_history_instances(
        &self,
        _target: &ContextTarget,
        _entity_ids: &[Identifier],
        _scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("count_history_instances"))
    }

    async fn delete_history_instances(
        &self,
        _target: &ContextTarget,
        _entity_ids: &[Identifier],
        _scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("delete_history_instances"))
    }
}

impl ContextCommitter for StubBroker {
    fn commit(&self, _target: &ContextTarget, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("commit is not exercised by the traversal harness") })
    }

    fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("commit_replayed is not exercised by the traversal harness") })
    }
}

/// A service whose every port is the same stubbed broker, joining purely
/// against local history.
fn service(broker: StubBroker) -> TemporalService {
    let broker = Arc::new(broker);
    TemporalService::new(
        TemporalServicePorts {
            entity_reader: Arc::clone(&broker) as Arc<dyn EntityReader>, // clone: one test double supplies every focused capability
            reader: Arc::clone(&broker) as Arc<dyn TemporalHistoryReader>,
            mutation_barrier: Arc::clone(&broker) as Arc<dyn TemporalMutationBarrier>,
            mutator: Arc::clone(&broker) as Arc<dyn TemporalMutator>,
            writes: Arc::clone(&broker) as Arc<dyn ContextCommitter>, // clone: one test double supplies every focused capability
            retry_delay: broker as Arc<dyn RetryDelay>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
        Arc::new(ChangeNotifier::new(None, SubscriptionMatching::Absent)),
        TemporalServiceLimits {
            instance_limits: TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000,
            },
            delete_by_query_max_matches: 100,
            aggregation_grouping_max_groups: 100,
            payload_budget: EntityConfig::default().payload_budget(),
        },
        None,
        TemporalJoin::Extended,
    )
}

/// A context mapping the harness's one term to a deterministic,
/// offline-resolvable IRI, so compaction needs no network fetch.
fn context() -> ContextSpec {
    ContextSpec::from_body(json!({
        "linksTo": "https://example.com/linksTo",
        "reading": "https://example.com/reading",
    }))
}

/// A flat join `join_level` levels deep over all history, normalized.
fn flat_join(join_level: u32) -> TemporalQueryParameters {
    let mut params = serde_json::from_value::<TemporalQueryParameters>(json!({})).expect("empty temporal query");
    params.timerel = Some(TimeRel::After);
    params.time_at = Some("1970-01-01T00:00:00Z".parse().expect("valid instant"));
    params.format = Some(WrittenValue::Supported(TemporalFormat::Normalized));
    params.join = Some(JoinMode::Flat);
    params.join_level = Some(level(join_level));
    params
}

fn level(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("positive join level")
}

/// The root the traversal starts from, rendered the way the endpoints render
/// it before asking for the join.
fn root(id: &str, targets: &[&str]) -> Value {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    json!({"id": id, "type": "Thing", "linksTo": instances})
}

/// One recorded call into the remote Linked Entity source.
#[derive(Debug, PartialEq)]
struct RemoteCall {
    /// The identifier the traversal asked for.
    id: Identifier,
    /// The levels still to resolve, this one included.
    remaining_depth: NonZeroU32,
    /// The already-walked identifiers handed over as `containedBy`.
    visited: Vec<Identifier>,
}

/// What the stubbed remainder of the system answers with.
#[derive(Debug)]
enum RemoteOutcome {
    /// The temporal representations each requested identifier resolves to.
    Returns(HashMap<Identifier, Vec<Value>>),
    /// The remainder of the system cannot be consulted at all.
    Fails,
}

/// A [`RemoteTemporalLinkedSource`] that records every call it receives.
#[derive(Debug)]
struct RecordingSource {
    /// Every call, in the order it was made.
    calls: Mutex<Vec<RemoteCall>>,
    /// How far every holder resolves on its own.
    reach: RemoteReach,
    /// What every call answers with.
    outcome: RemoteOutcome,
}

impl RecordingSource {
    fn returning(reach: RemoteReach, entities: impl IntoIterator<Item = (Identifier, Vec<Value>)>) -> Arc<RecordingSource> {
        Arc::new(RecordingSource {
            calls: Mutex::new(Vec::new()),
            reach,
            outcome: RemoteOutcome::Returns(entities.into_iter().collect()),
        })
    }

    fn failing() -> Arc<RecordingSource> {
        Arc::new(RecordingSource {
            calls: Mutex::new(Vec::new()),
            reach: RemoteReach::Subgraph,
            outcome: RemoteOutcome::Fails,
        })
    }

    fn calls(&self) -> MutexGuard<'_, Vec<RemoteCall>> {
        self.calls.lock().expect("the recording lock is never poisoned")
    }
}

#[async_trait]
impl RemoteTemporalLinkedSource for RecordingSource {
    async fn remote_reach(&self, _id: &Identifier, _context: &ContextSpec) -> RemoteReach {
        self.reach
    }

    async fn fetch_linked_temporal(
        &self,
        id: &Identifier,
        remaining_depth: NonZeroU32,
        visited: &[Identifier],
        _window: &TemporalLinkedWindow,
        _context: &ContextSpec,
    ) -> Result<RemoteTemporalLinkedEntities, TemporalError> {
        self.calls().push(RemoteCall {
            id: id.clone(), // clone: the recording owns the identifier it reports
            remaining_depth,
            visited: visited.to_vec(),
        });
        match &self.outcome {
            RemoteOutcome::Returns(entities) => Ok(RemoteTemporalLinkedEntities {
                entities: entities.get(id).cloned().unwrap_or_default(), // clone: the source hands back owned values
                failures: Vec::new(),
            }),
            RemoteOutcome::Fails => Err(TemporalError::from(PersistenceFailure::Unavailable)),
        }
    }
}

/// A service whose history is the stubbed broker and whose non-local half is
/// the recording source.
fn service_with_remote(broker: StubBroker, source: &Arc<RecordingSource>) -> TemporalService {
    let broker = Arc::new(broker);
    TemporalService::new(
        TemporalServicePorts {
            entity_reader: Arc::clone(&broker) as Arc<dyn EntityReader>, // clone: one test double supplies every focused capability
            reader: Arc::clone(&broker) as Arc<dyn TemporalHistoryReader>,
            mutation_barrier: Arc::clone(&broker) as Arc<dyn TemporalMutationBarrier>,
            mutator: Arc::clone(&broker) as Arc<dyn TemporalMutator>,
            writes: Arc::clone(&broker) as Arc<dyn ContextCommitter>, // clone: one test double supplies every focused capability
            retry_delay: broker as Arc<dyn RetryDelay>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
        Arc::new(ChangeNotifier::new(None, SubscriptionMatching::Absent)),
        TemporalServiceLimits {
            instance_limits: TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000,
            },
            delete_by_query_max_matches: 100,
            aggregation_grouping_max_groups: 100,
            payload_budget: EntityConfig::default().payload_budget(),
        },
        Some(Arc::clone(source) as Arc<dyn RemoteTemporalLinkedSource>), // clone: the test keeps its handle on the recording
        TemporalJoin::Extended,
    )
}

/// A root whose Relationship annotates its objects with the clause 4.5.23.1
/// `objectType` that declares each one an Internal Linked Entity.
fn annotated_root(id: &str, targets: &[&str]) -> Value {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "objectType": "Thing", "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    json!({"id": id, "type": "Thing", "linksTo": instances})
}

/// The join `params` asks for from `roots`, which named `seed` before they
/// were rendered.
async fn joined(service: &TemporalService, roots: &[Value], seed: &[LinkedEdge], params: &TemporalQueryParameters) -> TemporalLinkedJoin {
    service
        .join_linked_temporal(&live_target(), roots, seed, params, &context())
        .await
        .expect("resolves")
        .expect("the request asked for a join")
}

/// The Linked Entities a flat join appends, each as it is written.
fn appended(joined: &TemporalLinkedJoin) -> Vec<Value> {
    match joined.plan() {
        JoinPlan::Flat(flat) => flat.linked().map(|linked| narrowed_temporal_entity(linked.node, linked.rule)).collect(),
        JoinPlan::Inline(_) => panic!("the tests join flat"),
    }
}

/// The identifiers of the Linked Entities a flat join appends, in order.
fn appended_ids(joined: &TemporalLinkedJoin) -> Vec<String> {
    joined.plan().linked_ids().into_iter().map(ToString::to_string).collect()
}

/// `@none` and an absent `join` retrieve no Linked Entity, so nothing is
/// traversed (clause 4.5.23).
#[tokio::test]
async fn no_retrieving_join_traverses_nothing() {
    let mut params = flat_join(1);
    params.join = Some(JoinMode::None);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let joined = service(StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]))
        .join_linked_temporal(&live_target(), &roots, &[], &params, &context())
        .await
        .expect("resolves");
    assert!(joined.is_none());
}

/// Clause 4.5.23: the Linked Entities are appended in the order they are
/// first encountered. A root naming C before B, whose B in turn names D,
/// resolves breadth-first as `[C, B]` then `[D]`.
#[tokio::test]
async fn breadth_first_traversal_keeps_first_encounter_order() {
    let broker = StubBroker::new([
        (uri("urn:ngsi-ld:T:C"), Vec::new()),
        (uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:D")]),
        (uri("urn:ngsi-ld:T:D"), Vec::new()),
    ]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &[], &flat_join(2)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D"]);
}

/// A cycle A -> B -> A terminates: the root is never a Linked Entity.
#[tokio::test]
async fn a_cycle_back_to_the_root_terminates() {
    let broker = StubBroker::new([
        (uri("urn:ngsi-ld:T:A"), vec![uri("urn:ngsi-ld:T:B")]),
        (uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:A")]),
    ]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &[], &flat_join(5)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.23 `containedBy`: with a `joinLevel`, an entity the client
/// already holds is not retrieved again.
#[tokio::test]
async fn contained_by_excludes_the_identifiers_it_lists() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new()), (uri("urn:ngsi-ld:T:C"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"])];
    let mut params = flat_join(1);
    params.contained_by = Some(vec![uri("urn:ngsi-ld:T:B")]);

    let joined = joined(&service(broker), &roots, &[], &params).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:C"]);
}

/// `joinLevel=1` follows exactly one hop.
#[tokio::test]
async fn join_level_one_stops_after_the_first_hop() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:D")]), (uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &[], &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// An id nothing backs is simply absent: clause 4.5.23 leaves the
/// Relationship that named it a plain URI rather than failing the retrieval.
#[tokio::test]
async fn an_unresolvable_identifier_is_absent_rather_than_fatal() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:Missing", "urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &[], &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
    pretty_assertions::assert_eq!(joined.failures(), &[] as &[LinkedResolutionFailure]);
}

/// Clause 4.5.23.1: an annotated id this broker records no history for is
/// asked of a holder that resolves its own sub-graph with the levels still to
/// resolve, and with the already-walked identifiers.
#[tokio::test]
async fn a_subgraph_holder_is_asked_for_the_remaining_depth_and_the_visited_ids() {
    let source = RecordingSource::returning(
        RemoteReach::Subgraph,
        [(uri("urn:ngsi-ld:T:B"), vec![json!({"id": "urn:ngsi-ld:T:B", "type": "Thing"})])],
    );
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &[], &flat_join(2)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
    pretty_assertions::assert_eq!(
        *source.calls(),
        vec![RemoteCall {
            id: uri("urn:ngsi-ld:T:B"),
            remaining_depth: level(2),
            visited: vec![uri("urn:ngsi-ld:T:A")],
        }]
    );
}

/// A holder that did not advertise the temporal join is asked for the entity
/// alone, and the Relationships of what it answers are walked here: B is
/// fetched for one level, and the C it names is then fetched on its own.
#[tokio::test]
async fn a_single_entity_holder_is_asked_for_one_level_and_walked_here() {
    let source = RecordingSource::returning(
        RemoteReach::SingleEntity,
        [
            (uri("urn:ngsi-ld:T:B"), vec![annotated_root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"])]),
            (uri("urn:ngsi-ld:T:C"), vec![json!({"id": "urn:ngsi-ld:T:C", "type": "Thing"})]),
        ],
    );
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &[], &flat_join(2)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    let asked: Vec<(String, u32)> = source.calls().iter().map(|call| (call.id.to_string(), call.remaining_depth.get())).collect();
    pretty_assertions::assert_eq!(asked, vec![("urn:ngsi-ld:T:B".to_owned(), 1), ("urn:ngsi-ld:T:C".to_owned(), 1)]);
}

/// Clause 4.5.23.1 gates non-local resolution on the `objectType`
/// annotation: an unannotated target this broker does not hold stays a plain
/// URI rather than becoming a forwarded retrieval.
#[tokio::test]
async fn an_absent_unannotated_candidate_is_never_fetched_remotely() {
    let source = RecordingSource::returning(
        RemoteReach::Subgraph,
        [(uri("urn:ngsi-ld:T:B"), vec![json!({"id": "urn:ngsi-ld:T:B", "type": "Thing"})])],
    );
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &[], &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), Vec::<String>::new());
    pretty_assertions::assert_eq!(joined.failures(), &[] as &[LinkedResolutionFailure]);
    assert!(source.calls().is_empty());
}

/// A remainder of the system that cannot be consulted degrades the join
/// rather than the request (clause 4.5.23), and says why so the caller can
/// raise a clause 6.3.17 warning.
#[tokio::test]
async fn a_failing_remote_source_reports_an_unreachable_failure() {
    let source = RecordingSource::failing();
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &[], &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), Vec::<String>::new());
    pretty_assertions::assert_eq!(joined.failures().len(), 1);
    pretty_assertions::assert_eq!(joined.failures()[0].id, uri("urn:ngsi-ld:T:B"));
    pretty_assertions::assert_eq!(joined.failures()[0].kind, LinkedResolutionFailureKind::Unreachable);
}

/// The sub-graph a holder returns around a fetched candidate was resolved
/// with the depth that holder was given: it joins the result but is not
/// walked again, so the D that C names is out of range and C is never
/// fetched.
#[tokio::test]
async fn a_remote_holders_extra_entities_are_neither_re_fetched_nor_expanded() {
    let source = RecordingSource::returning(
        RemoteReach::Subgraph,
        [(
            uri("urn:ngsi-ld:T:B"),
            vec![
                annotated_root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]),
                annotated_root("urn:ngsi-ld:T:C", &["urn:ngsi-ld:T:D"]),
            ],
        )],
    );
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(broker, &source), &roots, &[], &flat_join(2)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    pretty_assertions::assert_eq!(source.calls().iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
}

/// The edges a root named before clause 4.5.8 collapsed its instances into a
/// value array.
fn seed() -> [LinkedEdge; 1] {
    [LinkedEdge {
        from: uri("urn:ngsi-ld:T:A"),
        attribute: "linksTo".parse::<AttributeName>().expect("valid attribute name"),
        target: uri("urn:ngsi-ld:T:B"),
        eligibility: RemoteEligibility::LocalOnly,
    }]
}

/// A root as `temporalValues` renders it: no `object` member anywhere.
fn projected_root() -> Value {
    json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "linksTo": {"type": "Relationship", "values": [["urn:ngsi-ld:T:B", "2024-01-01T00:00:00Z"]]}
    })
}

/// Clause 4.5.8 erases the `object` members a join follows; the edges read
/// off the normalized form before that projection still lead the join.
#[tokio::test]
async fn the_seed_resolves_links_the_projected_roots_no_longer_carry() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);

    let joined = joined(&service(broker), &[projected_root()], &seed(), &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.21: a root whose list removes `id` is rendered without it, and
/// the edges its normalized form reported still identify it and lead the
/// join.
#[tokio::test]
async fn a_root_rendered_without_its_id_still_leads_the_join() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let unnamed = json!({"linksTo": [{"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]});
    let mut params = flat_join(1);
    params.pick = Some(Vec::from("linksTo,reading".parse::<ProjectionList>().expect("valid projection list")));

    let joined = joined(&service(broker), &[unnamed], &seed(), &params).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.21 under `temporalValues`: `linksTo{reading}` narrows B to
/// `reading`, and the group names no `id`, so B's identity is not written
/// either (D3).
#[tokio::test]
async fn a_nested_pick_narrows_the_linked_entity_under_temporal_values() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:C")])]);
    let mut params = flat_join(1);
    params.format = Some(WrittenValue::Supported(TemporalFormat::TemporalValues));
    params.pick = Some(Vec::from("id,linksTo{reading}".parse::<ProjectionList>().expect("valid projection list")));

    let joined = joined(&service(broker), &[projected_root()], &seed(), &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    assert!(appended[0].get("reading").is_some(), "the group keeps `reading`: {}", appended[0]);
    assert!(appended[0].get("linksTo").is_none(), "the group drops B's own `linksTo`: {}", appended[0]);
    assert!(appended[0].get("id").is_none(), "the group names no `id`: {}", appended[0]);
}

/// Clause 4.21: `omit=linksTo{reading}` keeps `linksTo` on the root and omits
/// `reading` inside what it links to, leaving the rest of B in place.
#[tokio::test]
async fn a_nested_omit_narrows_the_resolved_target() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let mut params = flat_join(1);
    params.omit = Some(Vec::from("linksTo{reading}".parse::<ProjectionList>().expect("valid projection list")));

    let joined = joined(&service(broker), &roots, &[], &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    assert!(appended[0].get("reading").is_none(), "the nested omit drops `reading`: {}", appended[0]);
    pretty_assertions::assert_eq!(appended[0]["id"], "urn:ngsi-ld:T:B");
}

/// D3 and Table 6.19.3.1-1: `attrs` is the `pick` synonym that keeps the
/// identity, and it narrows every entity of the payload, so a Linked Entity
/// holding none of the named attributes is written with its identity alone.
#[tokio::test]
async fn attrs_narrow_a_linked_entity_to_its_identity_when_it_has_none_of_them() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let mut params = flat_join(1);
    params.attrs = Some(vec!["linksTo".parse().expect("valid attribute name")]);

    let joined = joined(&service(broker), &roots, &[], &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    let mut kept: Vec<&str> = appended[0].as_object().expect("an entity").keys().map(String::as_str).collect();
    kept.sort_unstable();
    pretty_assertions::assert_eq!(kept, vec!["id", "type"]);
}

/// The stubbed broker's commits never lose, so it never waits between attempts.
#[async_trait]
impl RetryDelay for StubBroker {
    async fn wait(&self, _delay: Duration) {}
}
