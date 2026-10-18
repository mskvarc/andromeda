use crate::{
    outcome::ForwardedTemporalEntities,
    temporal_result::{merge_temporal_query_results, merge_temporal_retrieve_results},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::{
    received::ReceivedElement,
    registration::forwarding_mode::ForwardingMode,
    temporal::{received_temporal::ReceivedTemporalEntities, temporal_body::TemporalBody, temporal_history::TemporalHistory},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The holder's history of the entity `document` names, in the expanded form
/// the context-independent merge works on.
async fn history(document: Value) -> TemporalHistory {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
    let context = ContextSpec::from_body(
        JsonLiteral::admit(&json!({"speed": "https://example.com/speed", "fuel": "https://example.com/fuel"}).to_string()).expect("an admissible literal"),
    );
    let ReceivedTemporalEntities(mut elements) = ReceivedTemporalEntities::from_slice(document.to_string().as_bytes()).expect("an answer");
    let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
        panic!("the answer is a temporal entity: {document}");
    };
    let TemporalBody::Evolution(entity) = contextual.body else {
        panic!("the answer is a Temporal Evolution");
    };
    let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
    TemporalHistory::Instances(expansion.temporal(entity).await.expect("the answer expands"))
}

fn forwarded(mode: ForwardingMode, entities: Vec<TemporalHistory>) -> ForwardedTemporalEntities {
    ForwardedTemporalEntities {
        registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:test".parse().expect("an identifier")),
        mode,
        entities,
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }
}

fn id_of(history: &TemporalHistory) -> &str {
    history.id().map(Identifier::as_str).expect("the history names its entity")
}

fn instances(history: &TemporalHistory, attribute: &str) -> usize {
    let TemporalHistory::Instances(document) = history else {
        panic!("a history of instances");
    };
    document.instances().filter(|(name, _)| name.as_str() == attribute).count()
}

const SPEED: &str = "https://example.com/speed";

const FUEL: &str = "https://example.com/fuel";

/// An observation of `member` at `day` of January 2024.
fn observed(value: u32, day: u32, instance: Option<&str>) -> Value {
    let mut instance_member = json!({"type": "Property", "value": value, "observedAt": format!("2024-01-{day:02}T00:00:00Z")});
    if let Some(id) = instance {
        instance_member["instanceId"] = json!(id);
    }
    instance_member
}

/// Clause 4.3.6.2: inclusive sources are deduplicated by `instanceId`, so an
/// instance both holders answered counts once and the one only the remote
/// holds is added.
#[tokio::test]
async fn inclusive_sources_are_deduplicated_by_instance_id() {
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, Some("urn:i1"))]})).await;
    let remote = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, Some("urn:i1")), observed(20, 2, Some("urn:i2"))]})).await;
    let result = merge_temporal_query_results(vec![local], vec![forwarded(ForwardingMode::Inclusive, vec![remote])], TimeProp::Observed);
    assert_eq!(result.len(), 1);
    assert_eq!(instances(&result[0], SPEED), 2);
}

/// Clause 5.7.3.4: an auxiliary source adds an instance only when no instance
/// of its attribute is observed at the same instant; and an entity only an
/// auxiliary source holds is not added.
#[tokio::test]
async fn auxiliary_instances_fill_only_missing_instants() {
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, None)]})).await;
    let supplement =
        history(json!({"id": "urn:a", "type": "T", "speed": [observed(99, 1, None), observed(20, 2, None)], "fuel": [observed(50, 1, None)]})).await;
    let unknown = history(json!({"id": "urn:b", "type": "T", "speed": [observed(20, 1, None)]})).await;
    let result = merge_temporal_query_results(
        vec![local],
        vec![forwarded(ForwardingMode::Auxiliary, vec![supplement, unknown])],
        TimeProp::Observed,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(id_of(&result[0]), "urn:a");
    assert_eq!(instances(&result[0], SPEED), 2);
    assert_eq!(instances(&result[0], FUEL), 1);
}

/// Instances of one attribute a remote holder adds are listed oldest first.
#[tokio::test]
async fn merged_instances_are_listed_oldest_first() {
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [observed(3, 3, Some("urn:i3"))]})).await;
    let remote = history(json!({"id": "urn:a", "type": "T", "speed": [observed(1, 1, Some("urn:i1"))]})).await;
    let result = merge_temporal_query_results(vec![local], vec![forwarded(ForwardingMode::Inclusive, vec![remote])], TimeProp::Observed);
    let TemporalHistory::Instances(document) = &result[0] else {
        panic!("a history of instances");
    };
    let instants: Vec<_> = document
        .instances()
        .filter_map(|(_, instance)| instance.instant_on(TimeProp::Observed))
        .collect();
    assert!(instants.windows(2).all(|pair| pair[0] <= pair[1]), "{instants:?}");
}

/// An entity only an inclusive source holds is added to the result.
#[tokio::test]
async fn a_remote_entity_the_broker_does_not_hold_is_added() {
    let remote = history(json!({"id": "urn:b", "type": "T", "speed": [observed(20, 1, None)]})).await;
    let result = merge_temporal_query_results(Vec::new(), vec![forwarded(ForwardingMode::Inclusive, vec![remote])], TimeProp::Observed);
    assert_eq!(result.iter().map(id_of).collect::<Vec<_>>(), vec!["urn:b"]);
}

/// Clause 5.7.3.4: a retrieval combines the attributes every holder answered.
#[tokio::test]
async fn merge_temporal_retrieve_combines_remote_attributes() {
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, Some("urn:i1"))]})).await;
    let remote = history(json!({"id": "urn:a", "type": "T", "fuel": [observed(50, 1, None)]})).await;
    let merged = merge_temporal_retrieve_results(Some(local), vec![forwarded(ForwardingMode::Inclusive, vec![remote])], TimeProp::Observed).expect("an entity");
    assert_eq!(instances(&merged, SPEED), 1);
    assert_eq!(instances(&merged, FUEL), 1);
}

/// A retrieval answered by a remote holder alone is that holder's history; an
/// auxiliary holder alone answers nothing.
#[tokio::test]
async fn a_retrieval_answered_by_one_remote_holder_is_its_history() {
    let entity = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, None)]})).await;
    assert!(merge_temporal_retrieve_results(None, vec![forwarded(ForwardingMode::Inclusive, vec![entity.clone()])], TimeProp::Observed).is_some());
    assert!(merge_temporal_retrieve_results(None, vec![forwarded(ForwardingMode::Auxiliary, vec![entity])], TimeProp::Observed).is_none());
}

/// Clause 4.5.7: a source's instance with no `observedAt` has no place in a
/// read on `observedAt`, whether the source's history joins a held one or is
/// the only one, in a query and in a retrieval.
#[tokio::test]
async fn a_source_instance_without_the_requested_temporal_property_is_left_out() {
    let unobserved = json!({"type": "Property", "value": 999});
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [observed(10, 1, None)]})).await;
    let joining = history(json!({"id": "urn:a", "type": "T", "speed": [observed(20, 2, None), unobserved.clone()]})).await;
    let alone = history(json!({"id": "urn:b", "type": "T", "speed": [unobserved.clone(), observed(30, 3, None)]})).await;

    let result = merge_temporal_query_results(
        vec![local.clone()],
        vec![forwarded(ForwardingMode::Inclusive, vec![joining.clone(), alone.clone()])],
        TimeProp::Observed,
    );
    assert_eq!(result.iter().map(id_of).collect::<Vec<_>>(), vec!["urn:a", "urn:b"]);
    assert_eq!(instances(&result[0], SPEED), 2);
    assert_eq!(instances(&result[1], SPEED), 1);

    let merged =
        merge_temporal_retrieve_results(Some(local), vec![forwarded(ForwardingMode::Inclusive, vec![joining])], TimeProp::Observed).expect("an entity");
    assert_eq!(instances(&merged, SPEED), 2);
    let only_remote = merge_temporal_retrieve_results(None, vec![forwarded(ForwardingMode::Inclusive, vec![alone])], TimeProp::Observed).expect("an entity");
    assert_eq!(instances(&only_remote, SPEED), 1);
}

/// Clause 4.5.9 (T-D2): a read asked for `temporalValues` is gathered as
/// instances, so what two holders answer for one `datasetId` is the instances
/// of one dataset, which the writer then writes as one object of pairs.
#[tokio::test]
async fn merged_temporal_values_merge_pairs() {
    let dataset = |value: u32, day: u32| {
        let mut instance = observed(value, day, None);
        instance["datasetId"] = json!("urn:ngsi-ld:Dataset:a");
        instance
    };
    let local = history(json!({"id": "urn:a", "type": "T", "speed": [dataset(1, 1)]})).await;
    let remote = history(json!({"id": "urn:a", "type": "T", "speed": [dataset(2, 2)]})).await;
    let result = merge_temporal_query_results(vec![local], vec![forwarded(ForwardingMode::Inclusive, vec![remote])], TimeProp::Observed);
    let TemporalHistory::Instances(document) = &result[0] else {
        panic!("a history of instances");
    };
    let datasets: Vec<_> = document
        .instances()
        .map(|(_, instance)| instance.dataset_id().map(ToString::to_string))
        .collect();
    assert_eq!(datasets, vec![Some("urn:ngsi-ld:Dataset:a".to_owned()); 2]);
}
