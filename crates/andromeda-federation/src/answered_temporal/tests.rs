use crate::{
    fixed_registration_store::offline_service,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedResponse, ForwardedTemporalEntities, MatchedSources},
    received_temporal_answer::ReceivedTemporalAnswer,
    source_continuation::SourceContinuation,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp};
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, temporal::temporal_history::TemporalHistory};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::Arc;

fn registration() -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:1",
        "type": "ContextSourceRegistration",
        "endpoint": "https://peer.example.org",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}

fn context(terms: &Value) -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(&terms.to_string()).expect("an admissible literal"))
}

/// What one source answering `body` under `naming` contributes to a client
/// whose context is `client`, read into the model.
async fn answered(body: &str, naming: ContextSpec, client: &ContextSpec) -> ForwardedTemporalEntities {
    let registration = registration();
    let outcome = ForwardGetOutcome::Success(ForwardedResponse {
        body: Arc::from(body.as_bytes()),
        naming: Box::new(naming),
        warnings: Vec::new(),
        entity_map: None,
        continuation: SourceContinuation::Complete,
        partial_range: None,
    });
    let page = crate::FederationService::received_temporal_answer(&registration, outcome);
    let received: ForwardReadOutcome<ReceivedTemporalAnswer> = ForwardReadOutcome {
        results: page.answer.into_iter().collect(),
        sources: MatchedSources::default(),
    };
    let service = offline_service(Vec::new());
    service
        .answered_temporal(received, client, TimeProp::Observed)
        .await
        .results
        .pop()
        .expect("one answer")
}

fn ids(answer: &ForwardedTemporalEntities) -> Vec<String> {
    answer.entities.iter().filter_map(|history| history.id().map(ToString::to_string)).collect()
}

fn speed_instances(history: &TemporalHistory) -> usize {
    let TemporalHistory::Instances(document) = history else {
        panic!("a history of instances");
    };
    document.instances().filter(|(name, _)| name.as_str().ends_with("speed")).count()
}

/// An answer spelled under the core context reads every name expanded.
#[tokio::test]
async fn a_received_temporal_entity_is_expanded_under_its_source_context() {
    let answer = answered(
        r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","value":50,"observedAt":"2020-01-01T00:00:00Z"}]}]"#,
        ContextSpec::core_only(),
        &ContextSpec::core_only(),
    )
    .await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
    assert_eq!(speed_instances(&answer.entities[0]), 1);
    assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
}

/// An entity carrying its own `@context` is read under it, not under the
/// context the answer is spelled in.
#[tokio::test]
async fn the_entity_context_wins_over_the_forwarding_context() {
    let answer = answered(
        r#"[{"@context":{"spd":"https://x.example/speed"},"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","spd":[{"type":"Property","value":50,"observedAt":"2020-01-01T00:00:00Z"}]}]"#,
        ContextSpec::core_only(),
        &ContextSpec::core_only(),
    )
    .await;
    let TemporalHistory::Instances(document) = &answer.entities[0] else {
        panic!("a history of instances");
    };
    assert!(document.instances().any(|(name, _)| name.as_str() == "https://x.example/speed"), "{document:?}");
}

/// The pages a source answered one entity in are put together, an instance
/// both pages carry counting once.
#[tokio::test]
async fn the_pages_of_one_entity_are_put_together() {
    let page = |instances: &str| format!(r#"{{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{instances}]}}"#);
    let first = r#"{"type":"Property","value":1,"observedAt":"2020-01-01T00:00:00Z","instanceId":"urn:ngsi-ld:i:1"}"#;
    let second = r#"{"type":"Property","value":2,"observedAt":"2020-01-02T00:00:00Z","instanceId":"urn:ngsi-ld:i:2"}"#;
    let body = format!("[{},{}]", page(first), page(&format!("{first},{second}")));
    let answer = answered(&body, ContextSpec::core_only(), &ContextSpec::core_only()).await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
    assert_eq!(speed_instances(&answer.entities[0]), 2);
}

/// An aggregate names no IRI, so it is respelled: each name is expanded under
/// the context the source spelled it in and compacted under the client's.
#[tokio::test]
async fn an_aggregate_is_respelled_in_the_clients_terms() {
    let answer = answered(
        r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","spd":{"type":"Property","avg":[[2.5,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"]]}}]"#,
        context(&json!({"spd": "https://x.example/speed"})),
        &context(&json!({"speed": "https://x.example/speed"})),
    )
    .await;
    let TemporalHistory::Aggregates(entity) = &answer.entities[0] else {
        panic!("a history of aggregates");
    };
    assert_eq!(entity.attrs.keys().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["speed"]);
}

/// Decision F2: an entity the model cannot hold is refused with a clause 6.3.17
/// warning, the others kept.
#[tokio::test]
async fn an_invalid_temporal_entity_is_refused_with_a_warning() {
    let answer = answered(
        r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property"}]},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle"}]"#,
        ContextSpec::core_only(),
        &ContextSpec::core_only(),
    )
    .await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:2"]);
    assert!(
        matches!(answer.warnings.as_slice(), [NgsiLdWarning::RevalidationFailed(_)]),
        "{:?}",
        answer.warnings
    );
}

/// Clause 6.3.17: a `unitCode` a write would refuse is not relayed either.
#[tokio::test]
async fn a_temporal_entity_with_an_unusable_unit_code_is_refused() {
    let answer = answered(
        r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","value":50,"unitCode":"km/h","observedAt":"2020-01-01T00:00:00Z"}]}]"#,
        ContextSpec::core_only(),
        &ContextSpec::core_only(),
    )
    .await;
    assert_eq!(ids(&answer), Vec::<String>::new());
    assert_eq!(answer.warnings.len(), 1, "{:?}", answer.warnings);
}

/// A body that is no temporal entity document is a warning, not a failure.
#[tokio::test]
async fn a_payload_that_is_not_json_is_a_warning() {
    let answer = answered("<html>", ContextSpec::core_only(), &ContextSpec::core_only()).await;
    assert_eq!(answer.entities.len(), 0);
    assert!(
        matches!(answer.warnings.as_slice(), [NgsiLdWarning::RevalidationFailed(_)]),
        "{:?}",
        answer.warnings
    );
}
