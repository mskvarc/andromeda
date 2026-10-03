//! Traversal and candidate-collection behaviour of the temporal join.

use crate::{
    error::TemporalError,
    instance_limits::TemporalInstanceLimits,
    linked::{
        TemporalJoinRequest,
        candidates::{RemoteEligibility, TemporalLinkedCandidate, collect_candidates},
    },
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
    projection::grammar::parse_projection_list,
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, history_delete::TemporalHistoryDeleteScope},
};
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use indexmap::IndexSet;
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

fn attribute_name(value: &str) -> AttributeName {
    value.parse().expect("valid attribute name")
}

fn ids(candidates: &[TemporalLinkedCandidate]) -> Vec<&str> {
    candidates.iter().map(|candidate| candidate.id.as_str()).collect()
}

fn collect(entity: &Value) -> Vec<TemporalLinkedCandidate> {
    collect_candidates(iter::once(entity), &IndexSet::new())
}

/// A temporal representation carries one Relationship instance per
/// observation, so an attribute that pointed somewhere different at two
/// instants names two Linked Entities. Both are collected, in the order the
/// instances appear.
#[test]
fn two_relationship_instances_naming_two_objects_collect_both_in_order() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:C", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-02T00:00:00Z"}
        ]
    });

    let candidates = collect(&entity);

    pretty_assertions::assert_eq!(ids(&candidates), vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]);
}

/// An entity that stayed parked at the same place across two observations is
/// one Linked Entity, not two: the duplicate is collapsed into the entry
/// already recorded for it.
#[test]
fn a_repeated_object_is_collected_once() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-02T00:00:00Z"}
        ]
    });

    let candidates = collect(&entity);

    pretty_assertions::assert_eq!(ids(&candidates), vec!["urn:ngsi-ld:T:B"]);
}

/// A `ListRelationship` names its objects in `objectList`, and the list's order
/// is the order they are collected in.
#[test]
fn an_object_list_is_collected_in_list_order() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "hasParts": [{
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"],
            "observedAt": "2024-01-01T00:00:00Z"
        }]
    });

    let candidates = collect(&entity);

    pretty_assertions::assert_eq!(ids(&candidates), vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]);
}

/// A single-instance attribute may reach the walk as a bare object rather than
/// a one-element array, and names its Linked Entity just the same.
#[test]
fn a_single_instance_rendered_as_a_bare_object_is_walked() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "parkedAt": {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}
    });

    pretty_assertions::assert_eq!(ids(&collect(&entity)), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.23.1: only a Relationship annotated with an `objectType` declares
/// its object an Internal Linked Entity, which is what admits a non-local
/// resolution. An unannotated one stays local.
#[test]
fn the_object_type_annotation_is_what_admits_a_non_local_resolution() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "annotated": [{"type": "Relationship", "object": "urn:ngsi-ld:T:R", "objectType": "Thing"}],
        "plain": [{"type": "Relationship", "object": "urn:ngsi-ld:T:L"}]
    });

    let candidates = collect(&entity);

    pretty_assertions::assert_eq!(
        candidates,
        vec![
            TemporalLinkedCandidate {
                id: uri("urn:ngsi-ld:T:R"),
                eligibility: RemoteEligibility::Eligible,
                attributes: vec![attribute_name("annotated")],
            },
            TemporalLinkedCandidate {
                id: uri("urn:ngsi-ld:T:L"),
                eligibility: RemoteEligibility::LocalOnly,
                attributes: vec![attribute_name("plain")],
            },
        ]
    );
}

/// One annotated instance is enough: an id first met without an `objectType`
/// and later with one becomes eligible, rather than staying local because of
/// the order the instances happened to be in.
#[test]
fn a_later_annotated_instance_upgrades_an_already_collected_identifier() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "objectType": "Thing", "observedAt": "2024-01-02T00:00:00Z"}
        ]
    });

    let candidates = collect(&entity);

    pretty_assertions::assert_eq!(
        candidates,
        vec![TemporalLinkedCandidate {
            id: uri("urn:ngsi-ld:T:B"),
            eligibility: RemoteEligibility::Eligible,
            attributes: vec![attribute_name("parkedAt")],
        }]
    );
}

/// The structural members of a temporal entity are not attributes, so nothing
/// inside them is mistaken for a Linked Entity, and a Property naming a URI is
/// not a Relationship.
#[test]
fn structural_members_and_properties_name_no_linked_entity() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "@context": "https://example.com/context.jsonld",
        "createdAt": "2024-01-01T00:00:00Z",
        "reading": [{"type": "Property", "value": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]
    });

    pretty_assertions::assert_eq!(collect(&entity), Vec::<TemporalLinkedCandidate>::new());
}

/// The visited guard is what terminates a cycle: an identifier already walked,
/// or one the client said it already holds through `containedBy`, is never
/// collected again (clause 4.5.23).
#[test]
fn a_visited_identifier_is_never_collected_again() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:B",
        "type": "Thing",
        "parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:A"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:C"}
        ]
    });
    let visited: IndexSet<Identifier> = [uri("urn:ngsi-ld:T:A")].into_iter().collect();

    let candidates = collect_candidates(iter::once(&entity), &visited);

    pretty_assertions::assert_eq!(ids(&candidates), vec!["urn:ngsi-ld:T:C"]);
}

/// An `object` that is not a URI at all degrades rather than failing the
/// retrieval: it names no Linked Entity and the rest of the walk continues.
#[test]
fn an_unusable_object_value_names_no_linked_entity() {
    let entity = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "T",
        "broken": [{"type": "Relationship", "object": "not a uri"}],
        "partly": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B", 7]}]
    });
    pretty_assertions::assert_eq!(ids(&collect(&entity)), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.22.2 renders a normalized `objectList` as `{"object": "urn:…"}`
/// members and clause 4.5.3.2 renders a multi-target `object` as an array of
/// URIs, so the traversal finds its Linked Entities in both, in the order the
/// members declare them.
#[test]
fn a_normalized_object_list_and_a_multi_target_object_both_name_linked_entities() {
    let list_relationship = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "T",
        "route": [{
            "type": "ListRelationship",
            "objectList": [{"object": "urn:ngsi-ld:T:B"}, {"object": "urn:ngsi-ld:T:C"}]
        }]
    });
    pretty_assertions::assert_eq!(ids(&collect(&list_relationship)), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);

    let multi_target = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "T",
        "passengers": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:D", "urn:ngsi-ld:T:E"]}]
    });
    pretty_assertions::assert_eq!(ids(&collect(&multi_target)), vec!["urn:ngsi-ld:T:D", "urn:ngsi-ld:T:E"]);
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

/// The window every traversal test runs over: all history, normalized.
fn window() -> TemporalQueryParameters {
    let mut params = serde_json::from_value::<TemporalQueryParameters>(json!({})).expect("empty temporal query");
    params.timerel = Some(TimeRel::After);
    params.time_at = Some("1970-01-01T00:00:00Z".parse().expect("valid instant"));
    params.format = Some(WrittenValue::Supported(TemporalFormat::Normalized));
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

async fn resolve(broker: StubBroker, roots: &[Value], join_level: NonZeroU32, contained_by: &HashSet<Identifier>) -> Vec<String> {
    let resolved = service(broker)
        .resolve_linked_temporal(
            &live_target(),
            roots,
            &[],
            &TemporalJoinRequest {
                params: &window(),
                join_level,
                contained_by,
                context: &context(),
            },
        )
        .await
        .expect("resolves");
    resolved.entities.keys().map(ToString::to_string).collect()
}

/// Clause 4.5.23: the traversal appends linked entities in the order it
/// encounters them. A root naming C before B, whose B in turn names D,
/// resolves breadth-first as `[C, B]` then `[D]`.
#[tokio::test]
async fn breadth_first_traversal_keeps_first_encounter_order() {
    let broker = StubBroker::new([
        (uri("urn:ngsi-ld:T:C"), Vec::new()),
        (uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:D")]),
        (uri("urn:ngsi-ld:T:D"), Vec::new()),
    ]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"])];

    let resolved = resolve(broker, &roots, level(2), &HashSet::new()).await;

    pretty_assertions::assert_eq!(resolved, vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D"]);
}

/// A cycle A -> B -> A terminates: the visited guard excludes the root, so B
/// is resolved once and its back-reference resolves nothing.
#[tokio::test]
async fn a_cycle_back_to_the_root_terminates() {
    let broker = StubBroker::new([
        (uri("urn:ngsi-ld:T:A"), vec![uri("urn:ngsi-ld:T:B")]),
        (uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:A")]),
    ]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = resolve(broker, &roots, level(5), &HashSet::new()).await;

    pretty_assertions::assert_eq!(resolved, vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.23 `containedBy`: an entity the client already holds is not
/// retrieved again, and is not descended into.
#[tokio::test]
async fn contained_by_excludes_the_identifiers_it_lists() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new()), (uri("urn:ngsi-ld:T:C"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"])];
    let contained_by: HashSet<Identifier> = [uri("urn:ngsi-ld:T:B")].into_iter().collect();

    let resolved = resolve(broker, &roots, level(1), &contained_by).await;

    pretty_assertions::assert_eq!(resolved, vec!["urn:ngsi-ld:T:C"]);
}

/// `joinLevel=1` follows exactly one hop: B is resolved, the D it links to is
/// not.
#[tokio::test]
async fn join_level_one_stops_after_the_first_hop() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:D")]), (uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = resolve(broker, &roots, level(1), &HashSet::new()).await;

    pretty_assertions::assert_eq!(resolved, vec!["urn:ngsi-ld:T:B"]);
}

/// An id nothing backs is simply absent: clause 4.5.23 leaves the Relationship
/// that named it a plain URI rather than failing the retrieval.
#[tokio::test]
async fn an_unresolvable_identifier_is_absent_rather_than_fatal() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:Missing", "urn:ngsi-ld:T:B"])];

    let resolved = resolve(broker, &roots, level(1), &HashSet::new()).await;

    pretty_assertions::assert_eq!(resolved, vec!["urn:ngsi-ld:T:B"]);
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
    /// What every call answers with.
    outcome: RemoteOutcome,
}

impl RecordingSource {
    fn returning(entities: impl IntoIterator<Item = (Identifier, Vec<Value>)>) -> Arc<RecordingSource> {
        Arc::new(RecordingSource {
            calls: Mutex::new(Vec::new()),
            outcome: RemoteOutcome::Returns(entities.into_iter().collect()),
        })
    }

    fn failing() -> Arc<RecordingSource> {
        Arc::new(RecordingSource {
            calls: Mutex::new(Vec::new()),
            outcome: RemoteOutcome::Fails,
        })
    }

    fn calls(&self) -> MutexGuard<'_, Vec<RemoteCall>> {
        self.calls.lock().expect("the recording lock is never poisoned")
    }
}

#[async_trait]
impl RemoteTemporalLinkedSource for RecordingSource {
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

/// Clause 4.5.23.1: an annotated id this broker records no history for is asked
/// of the remainder of the system — with the levels still to resolve, and with
/// the already-walked identifiers so the remote side does not send back what
/// the requester already holds.
#[tokio::test]
async fn an_absent_annotated_candidate_is_fetched_with_the_remaining_depth_and_the_visited_ids() {
    let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![json!({"id": "urn:ngsi-ld:T:B", "type": "Thing"})])]);
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = service_with_remote(StubBroker::new([]), &source)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &window(),
                join_level: level(2),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(resolved.entities.keys().map(Identifier::as_str).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
    pretty_assertions::assert_eq!(
        *source.calls(),
        vec![RemoteCall {
            id: uri("urn:ngsi-ld:T:B"),
            remaining_depth: level(2),
            visited: vec![uri("urn:ngsi-ld:T:A")],
        }]
    );
}

/// Clause 4.5.23.1 gates non-local resolution on the `objectType` annotation:
/// an unannotated Relationship names a plain URI, which stays a plain URI
/// rather than becoming a forwarded retrieval.
#[tokio::test]
async fn an_absent_unannotated_candidate_is_never_fetched_remotely() {
    let source = RecordingSource::returning([(uri("urn:ngsi-ld:T:B"), vec![json!({"id": "urn:ngsi-ld:T:B", "type": "Thing"})])]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = service_with_remote(StubBroker::new([]), &source)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &window(),
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    assert!(resolved.entities.is_empty());
    pretty_assertions::assert_eq!(resolved.failures, Vec::<LinkedResolutionFailure>::new());
    assert!(source.calls().is_empty());
}

/// A remainder of the system that cannot be consulted degrades the join rather
/// than the request (clause 4.5.23): the retrieval still succeeds, the id stays
/// unresolved, and the caller is told why so it can raise a clause 6.3.17
/// warning.
#[tokio::test]
async fn a_failing_remote_source_reports_an_unreachable_failure() {
    let source = RecordingSource::failing();
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = service_with_remote(StubBroker::new([]), &source)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &window(),
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves despite the unreachable remainder");

    assert!(resolved.entities.is_empty());
    pretty_assertions::assert_eq!(resolved.failures.len(), 1);
    pretty_assertions::assert_eq!(resolved.failures[0].id, uri("urn:ngsi-ld:T:B"));
    pretty_assertions::assert_eq!(resolved.failures[0].kind, LinkedResolutionFailureKind::Unreachable);
}

/// The sub-graph a remote broker returns around a fetched candidate is already
/// resolved, with the depth budget that broker was given. Those extra entities
/// join the result and the visited guard, but they are not walked again: `C`
/// arrives as `B`'s link at the second and last level, so the `D` it names is
/// out of range, and `C` itself is never fetched.
#[tokio::test]
async fn a_remote_brokers_extra_entities_are_neither_re_fetched_nor_expanded() {
    let source = RecordingSource::returning([(
        uri("urn:ngsi-ld:T:B"),
        vec![
            annotated_root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]),
            annotated_root("urn:ngsi-ld:T:C", &["urn:ngsi-ld:T:D"]),
        ],
    )]);
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let resolved = service_with_remote(broker, &source)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &window(),
                join_level: level(2),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(
        resolved.entities.keys().map(Identifier::as_str).collect::<Vec<_>>(),
        vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]
    );
    pretty_assertions::assert_eq!(source.calls().iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.8 collapses attribute instances into value arrays, which erases
/// the `object` members a join follows. The seed carries what the roots named,
/// read off the normalized form before that projection, so a flat join over
/// `temporalValues` still resolves — where walking the projected root alone
/// would find nothing.
#[tokio::test]
async fn a_seed_resolves_links_the_projected_roots_no_longer_carry() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    // A root as `temporalValues` renders it: no `object` member anywhere.
    let projected = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "linksTo": {"type": "Relationship", "values": [["urn:ngsi-ld:T:B", "2024-01-01T00:00:00Z"]]}
    });
    let seed = [TemporalLinkedCandidate {
        id: uri("urn:ngsi-ld:T:B"),
        eligibility: RemoteEligibility::LocalOnly,
        attributes: vec![attribute_name("linksTo")],
    }];

    let resolved = service(broker)
        .resolve_linked_temporal(
            &live_target(),
            &[projected],
            &seed,
            &TemporalJoinRequest {
                params: &window(),
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(resolved.entities.keys().map(Identifier::as_str).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.21 nested narrowing must not depend on the roots' *final*
/// rendering: `temporalValues` (clause 4.5.8) collapses an attribute's
/// instances into a value array, erasing the `object` member a walk over the
/// rendered root would need to associate a nested sub-projection with an
/// identifier. The nested projection is instead read off the candidate's
/// `attribute`, carried since the pre-transform normalized form, so the same
/// narrowing applies under `temporalValues` too.
#[tokio::test]
async fn a_nested_pick_narrows_the_linked_entity_under_temporal_values() {
    // `B` has an outgoing link of its own, so its rendering carries two
    // attributes -- `reading` and `linksTo` -- and the nested pick can be
    // shown to keep one while dropping the other.
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:C")])]);
    // A root as `temporalValues` renders it: no `object` member anywhere.
    let projected = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "linksTo": {"type": "Relationship", "values": [["urn:ngsi-ld:T:B", "2024-01-01T00:00:00Z"]]}
    });
    let seed = [TemporalLinkedCandidate {
        id: uri("urn:ngsi-ld:T:B"),
        eligibility: RemoteEligibility::LocalOnly,
        attributes: vec![attribute_name("linksTo")],
    }];
    let mut params = window();
    params.format = Some(WrittenValue::Supported(TemporalFormat::TemporalValues));
    params.pick = Some(parse_projection_list("linksTo{reading}").expect("valid projection list"));

    let resolved = service(broker)
        .resolve_linked_temporal(
            &live_target(),
            &[projected],
            &seed,
            &TemporalJoinRequest {
                params: &params,
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    let target = resolved.entities.get(&uri("urn:ngsi-ld:T:B")).expect("target resolved");
    assert!(target.get("reading").is_some(), "the nested pick keeps `reading`: {target}");
    assert!(target.get("linksTo").is_none(), "the nested pick drops B's own `linksTo`: {target}");
    assert_eq!(target["id"], "urn:ngsi-ld:T:B");
}

/// Clause 4.21: `omit=rel{a}` omits `a` inside whatever `rel` links to, not
/// `rel` itself -- the root-side half of that rule (the relationship surviving
/// on the root) is `TemporalEntityRenderer`'s
/// `a_nested_omit_term_still_keeps_its_relationship_on_the_root`. This is the
/// other half: the resolved target is narrowed, dropping `a` and leaving
/// everything else -- including its identity -- in place.
#[tokio::test]
async fn a_nested_omit_narrows_the_resolved_target() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let mut params = window();
    params.omit = Some(parse_projection_list("linksTo{reading}").expect("valid projection list"));

    let resolved = service(broker)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &params,
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    let target = resolved.entities.get(&uri("urn:ngsi-ld:T:B")).expect("target resolved");
    assert!(target.get("reading").is_none(), "the nested omit drops `reading` from the target: {target}");
    assert_eq!(target["id"], "urn:ngsi-ld:T:B");
}

/// The window carries into the traversal but the request's `attrs` does not: a
/// linked entity that has none of the attributes the client asked the roots for
/// still appears, rendered with its own.
#[tokio::test]
async fn a_linked_entity_lacking_the_requested_attrs_still_appears_with_its_own() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    // The roots were narrowed to an attribute the linked entity does not have;
    // `B` only ever records `reading` and `linksTo`.
    let mut narrowed = window();
    narrowed.attrs = Some(vec!["https://example.com/absent".parse().expect("valid attribute name")]);

    let resolved = service(broker)
        .resolve_linked_temporal(
            &live_target(),
            &roots,
            &[],
            &TemporalJoinRequest {
                params: &narrowed,
                join_level: level(1),
                contained_by: &HashSet::new(),
                context: &context(),
            },
        )
        .await
        .expect("resolves");

    pretty_assertions::assert_eq!(resolved.entities.keys().map(Identifier::as_str).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
    let linked = resolved.entities.get(&uri("urn:ngsi-ld:T:B")).expect("the linked entity");
    assert!(
        linked.get("reading").is_some() || linked.get("https://example.com/reading").is_some(),
        "the linked entity kept its own attributes: {linked}"
    );
}

/// The stubbed broker's commits never lose, so it never waits between attempts.
#[async_trait]
impl RetryDelay for StubBroker {
    async fn wait(&self, _delay: Duration) {}
}
