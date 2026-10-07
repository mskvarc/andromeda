//! The temporal join through the Linked Entity engine: what the temporal
//! source reads, what it asks the remainder of the system, and how the
//! request's list narrows what it resolved.

use crate::{
    error::TemporalError,
    linked::TemporalLinkedJoin,
    remote_linked::{RemoteTemporalLinkedEntities, RemoteTemporalLinkedSource, TemporalLinkedWindow},
    service::TemporalService,
    stub_broker::{StubBroker, live_target, service, service_over, uri},
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_linked_retrieval::{keyword::Keyword, plan::JoinPlan, representation::LinkedRepresentation, source::RemoteReach};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_rel::TimeRel};
use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_entity::TemporalEntity, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::projection::grammar::ProjectionList;
use andromeda_ngsi_ld_render::temporal::temporal_evolutions::TemporalEvolutions;
use andromeda_ngsi_ld_representation::{
    join_mode::JoinMode,
    linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind},
};
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use serde_json::{Value, json, value::RawValue};
use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard},
};

/// A context mapping the harness's one term to a deterministic,
/// offline-resolvable IRI, so compaction needs no network fetch.
fn context() -> ContextSpec {
    ContextSpec::from_body(
        JsonLiteral::admit(
            &json!({
                "linksTo": "https://example.com/linksTo",
                "reading": "https://example.com/reading",
            })
            .to_string(),
        )
        .expect("an admissible literal"),
    )
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

/// The compact Temporal Evolution `document` is.
fn body(document: &Value) -> TemporalBody {
    let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
    TemporalBody::Evolution(TemporalEntity::from_node(JsonPiece::new(&raw)).expect("a Temporal Evolution"))
}

/// The root the traversal starts from, compacted the way the endpoints compact
/// it before asking for the join.
fn root(id: &str, targets: &[&str]) -> TemporalBody {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    body(&json!({"id": id, "type": "Thing", "linksTo": instances}))
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
    Returns(HashMap<Identifier, Vec<TemporalHistory>>),
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
    fn returning(reach: RemoteReach, entities: impl IntoIterator<Item = (Identifier, Vec<TemporalHistory>)>) -> Arc<RecordingSource> {
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
    service_over(Arc::new(broker), Some(Arc::clone(source) as Arc<dyn RemoteTemporalLinkedSource>)) // clone: the test keeps its handle on the recording
}

/// A root whose Relationship annotates its objects with the clause 4.5.23.1
/// `objectType` that declares each one an Internal Linked Entity.
fn annotated_root(id: &str, targets: &[&str]) -> TemporalBody {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "objectType": "Thing", "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    body(&json!({"id": id, "type": "Thing", "linksTo": instances}))
}

/// What a remote holder answers for `body`: its history, in the form the
/// holder's answer is read into.
async fn remote_history(body: &TemporalBody) -> TemporalHistory {
    let TemporalBody::Evolution(entity) = body.clone() else {
        panic!("a remote holder answers with instances");
    };
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
    let expansion = EntityExpansion::prepare(&engine, &context()).await.expect("the context resolves");
    TemporalHistory::Instances(expansion.temporal(entity).await.expect("the answer expands"))
}

/// A Linked Entity holding nothing but its identity.
fn bare(id: &str) -> TemporalBody {
    body(&json!({"id": id, "type": "Thing"}))
}

/// The join `params` asks for from `roots`.
async fn joined(service: &TemporalService, roots: &[TemporalBody], params: &TemporalQueryParameters) -> TemporalLinkedJoin {
    service
        .join_linked_temporal(&live_target(), roots, params, &context())
        .await
        .expect("resolves")
        .expect("the request asked for a join")
}

/// The members the rule in force writes of each Linked Entity a flat join
/// appends: the keywords the entity carries, then its attributes.
fn appended(joined: &TemporalLinkedJoin) -> Vec<Vec<String>> {
    match joined.plan() {
        JoinPlan::Flat(flat) => flat
            .linked()
            .map(|linked| {
                let carried = TemporalEvolutions::keyword_members(linked.node);
                let mut members: Vec<String> = [("id", Keyword::Id), ("type", Keyword::Type), ("scope", Keyword::Scope)]
                    .into_iter()
                    .filter(|(_, keyword)| carried.contains(*keyword) && linked.rule.keyword(*keyword).is_write())
                    .map(|(key, _)| key.to_owned())
                    .collect();
                members.extend(
                    TemporalEvolutions::attribute_keys(linked.node)
                        .filter(|key| linked.rule.member(key).is_write())
                        .map(ToOwned::to_owned),
                );
                members
            })
            .collect(),
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
        .join_linked_temporal(&live_target(), &roots, &params, &context())
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

    let joined = joined(&service(broker), &roots, &flat_join(2)).await;

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

    let joined = joined(&service(broker), &roots, &flat_join(5)).await;

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

    let joined = joined(&service(broker), &roots, &params).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:C"]);
}

/// `joinLevel=1` follows exactly one hop.
#[tokio::test]
async fn join_level_one_stops_after_the_first_hop() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:D")]), (uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 5.6.6 removes the current state of an entity and leaves its Temporal
/// Evolution standing, so an entity with history and no current state is still
/// a Linked Entity: the temporal catalogue, not the current state, says which
/// entities have a Temporal Evolution here.
#[tokio::test]
async fn deleted_current_state_is_still_joined() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]).without_current_state();
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &flat_join(1)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B"]);
}

/// An id nothing backs is simply absent: clause 4.5.23 leaves the
/// Relationship that named it a plain URI rather than failing the retrieval.
#[tokio::test]
async fn an_unresolvable_identifier_is_absent_rather_than_fatal() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:Missing", "urn:ngsi-ld:T:B"])];

    let joined = joined(&service(broker), &roots, &flat_join(1)).await;

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
        [(uri("urn:ngsi-ld:T:B"), vec![remote_history(&bare("urn:ngsi-ld:T:B")).await])],
    );
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &flat_join(2)).await;

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
            (
                uri("urn:ngsi-ld:T:B"),
                vec![remote_history(&annotated_root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"])).await],
            ),
            (uri("urn:ngsi-ld:T:C"), vec![remote_history(&bare("urn:ngsi-ld:T:C")).await]),
        ],
    );
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &flat_join(2)).await;

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
        [(uri("urn:ngsi-ld:T:B"), vec![remote_history(&bare("urn:ngsi-ld:T:B")).await])],
    );
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &flat_join(1)).await;

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

    let joined = joined(&service_with_remote(StubBroker::new([]), &source), &roots, &flat_join(1)).await;

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
                remote_history(&annotated_root("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"])).await,
                remote_history(&annotated_root("urn:ngsi-ld:T:C", &["urn:ngsi-ld:T:D"])).await,
            ],
        )],
    );
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:D"), Vec::new())]);
    let roots = [annotated_root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];

    let joined = joined(&service_with_remote(broker, &source), &roots, &flat_join(2)).await;

    pretty_assertions::assert_eq!(appended_ids(&joined), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    pretty_assertions::assert_eq!(source.calls().iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.21 under `temporalValues`: `linksTo{reading}` narrows B to
/// `reading`, and the group names no `id`, so B's identity is not written
/// either.
#[tokio::test]
async fn a_nested_pick_narrows_the_linked_entity_under_temporal_values() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), vec![uri("urn:ngsi-ld:T:C")])]);
    let mut params = flat_join(1);
    params.format = Some(WrittenValue::Supported(TemporalFormat::TemporalValues));
    params.pick = Some(Vec::from("id,linksTo{reading}".parse::<ProjectionList>().expect("valid projection list")));

    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let joined = joined(&service(broker), &roots, &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    pretty_assertions::assert_eq!(appended[0], vec!["reading"], "the group keeps `reading` alone, and names no `id`");
}

/// Clause 4.21: `omit=linksTo{reading}` keeps `linksTo` on the root and omits
/// `reading` inside what it links to, leaving the rest of B in place.
#[tokio::test]
async fn a_nested_omit_narrows_the_resolved_target() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let mut params = flat_join(1);
    params.omit = Some(Vec::from("linksTo{reading}".parse::<ProjectionList>().expect("valid projection list")));

    let joined = joined(&service(broker), &roots, &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    pretty_assertions::assert_eq!(appended[0], vec!["id", "type"], "the nested omit drops `reading` and leaves the rest of B");
}

/// Table 6.19.3.1-1: `attrs` is the `pick` synonym that keeps the
/// identity, and it narrows every entity of the payload, so a Linked Entity
/// holding none of the named attributes is written with its identity alone.
#[tokio::test]
async fn attrs_narrow_a_linked_entity_to_its_identity_when_it_has_none_of_them() {
    let broker = StubBroker::new([(uri("urn:ngsi-ld:T:B"), Vec::new())]);
    let roots = [root("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"])];
    let mut params = flat_join(1);
    params.attrs = Some(vec!["linksTo".parse().expect("valid attribute name")]);

    let joined = joined(&service(broker), &roots, &params).await;

    let appended = appended(&joined);
    pretty_assertions::assert_eq!(appended.len(), 1);
    pretty_assertions::assert_eq!(appended[0], vec!["id", "type"]);
}
