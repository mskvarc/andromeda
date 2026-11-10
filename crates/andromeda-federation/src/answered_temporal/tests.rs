use crate::{
    fixed_registration_store::offline_service,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedResponse, ForwardedTemporalEntities, MatchedSources},
    received_temporal_answer::ReceivedTemporalAnswer,
    source_continuation::SourceContinuation,
    temporal_answer_reading::ReadTemporalEntity,
};
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
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
        partial_page: None,
    });
    let page = crate::FederationService::received_temporal_answer(&registration, outcome);
    let service = offline_service(Vec::new());
    let results = match page.answer {
        Some(answer) => vec![service.read_temporal_answer(answer).await],
        None => Vec::new(),
    };
    let received: ForwardReadOutcome<ReceivedTemporalAnswer<ReadTemporalEntity>> = ForwardReadOutcome {
        results,
        sources: MatchedSources::default(),
    };
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
        matches!(answer.warnings.as_slice(), [warning] if warning.code() == WarnCode::RevalidationFailed),
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

/// Clause 6.3.17: the evolutions a source answered that cannot be used are
/// reported with one warning for the source, counting every one of them
/// whichever stage of the reading left it out, and naming none.
#[tokio::test]
async fn three_unusable_temporal_entities_of_one_source_are_reported_with_one_warning_counting_them() {
    let answer = answered(
        r#"[
            {"id":"urn:ngsi-ld:Vehicle:valueless","type":"Vehicle","speed":[{"type":"Property"}]},
            {"id":"urn:ngsi-ld:Vehicle:kmh","type":"Vehicle","speed":[{"type":"Property","value":50,"unitCode":"km/h","observedAt":"2020-01-01T00:00:00Z"}]},
            {"@context":{"bad":{"@id":5}},"id":"urn:ngsi-ld:Vehicle:own-context","type":"Vehicle"},
            {"id":"urn:ngsi-ld:Vehicle:valid","type":"Vehicle"}
        ]"#,
        ContextSpec::core_only(),
        &ContextSpec::core_only(),
    )
    .await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:valid"]);
    assert_eq!(
        answer
            .warnings
            .iter()
            .map(|warning| (warning.code(), warning.text().as_str()))
            .collect::<Vec<_>>(),
        vec![(
            WarnCode::RevalidationFailed,
            "3 of the entities context source urn:ngsi-ld:ContextSourceRegistration:1 answered cannot be used"
        )]
    );
}

/// A body that is no temporal entity document is a warning, not a failure.
#[tokio::test]
async fn a_payload_that_is_not_json_is_a_warning() {
    let answer = answered("<html>", ContextSpec::core_only(), &ContextSpec::core_only()).await;
    assert_eq!(answer.entities.len(), 0);
    assert!(
        matches!(answer.warnings.as_slice(), [warning] if warning.code() == WarnCode::RevalidationFailed),
        "{:?}",
        answer.warnings
    );
}

/// The source context of the evolutions below: `kind` for `@type`, `vals`
/// for `ngsi-ld:hasValues`.
fn aliasing_terms() -> Value {
    json!({"kind": "@type", "vals": "https://uri.etsi.org/ngsi-ld/hasValues"})
}

/// Clauses 4.4 and 5.5.4: an evolution carrying its own aliasing `@context`
/// and written in the simplified temporal representation reads as the Property
/// it is, with no warning.
#[tokio::test]
async fn an_evolution_under_its_own_aliasing_context_reads_its_instances() {
    let body = json!([{
        "@context": aliasing_terms(),
        "id": "urn:ngsi-ld:Vehicle:1",
        "kind": "Vehicle",
        "speed": {"kind": "Property", "vals": [[20, "2026-01-01T00:00:00Z"], [21, "2026-01-01T01:00:00Z"]]}
    }]);
    let answer = answered(&body.to_string(), ContextSpec::core_only(), &ContextSpec::core_only()).await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
    assert_eq!(speed_instances(&answer.entities[0]), 2);
    assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
}

/// An evolution naming no `@context` is read under the aliases of the
/// forwarding context the answer is spelled in.
#[tokio::test]
async fn an_evolution_without_a_context_reads_under_the_forwarding_aliases() {
    let body = json!([{
        "id": "urn:ngsi-ld:Vehicle:1",
        "kind": "Vehicle",
        "speed": {"kind": "Property", "vals": [[20, "2026-01-01T00:00:00Z"]]}
    }]);
    let answer = answered(&body.to_string(), context(&aliasing_terms()), &ContextSpec::core_only()).await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
    assert_eq!(speed_instances(&answer.entities[0]), 1);
    assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
}

/// JSON-LD 1.1 section 4.6: a `@set` alias of `@type` writes each type as an
/// array of one, which reads as the type it holds.
#[tokio::test]
async fn an_evolution_under_a_set_container_reads_its_instances() {
    let body = json!([{
        "id": "urn:ngsi-ld:Vehicle:1",
        "kind": ["Vehicle"],
        "speed": [{"kind": ["Property"], "value": 20, "observedAt": "2026-01-01T00:00:00Z"}]
    }]);
    let naming = context(&json!({"kind": {"@id": "@type", "@container": "@set"}}));
    let answer = answered(&body.to_string(), naming, &ContextSpec::core_only()).await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
    assert_eq!(speed_instances(&answer.entities[0]), 1);
    assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
}

/// An evolution invalid under the context it was written in is still left out
/// with a clause 6.3.17 warning, its sibling kept.
#[tokio::test]
async fn an_evolution_invalid_under_its_aliasing_context_is_refused_with_a_warning() {
    let body = json!([
        {"id": "urn:ngsi-ld:Vehicle:1", "kind": "Vehicle", "speed": [{"kind": "Property", "observedAt": "2026-01-01T00:00:00Z"}]},
        {"id": "urn:ngsi-ld:Vehicle:2", "kind": "Vehicle", "speed": [{"kind": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}]}
    ]);
    let answer = answered(&body.to_string(), context(&aliasing_terms()), &ContextSpec::core_only()).await;
    assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:2"]);
    assert!(
        matches!(answer.warnings.as_slice(), [warning] if warning.code() == WarnCode::RevalidationFailed),
        "{:?}",
        answer.warnings
    );
}

/// What a peer's answer to a temporal query makes of every attribute member
/// shape written as the one instance of a history (clause 6.3.17): an entity
/// read typed keeps its sibling company, one refused is dropped with one
/// `111 Revalidation Failed` warning while its sibling is kept.
mod shapes {
    use crate::answered_temporal::tests::{answered, ids};
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_jsonld::context_spec::ContextSpec;
    use serde_json::{Value, json};

    /// What a route makes of a member written in one shape.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Verdict {
        /// The entity carrying the member is read and kept.
        Typed,
        /// The entity carrying the member is dropped with one `111 Revalidation
        /// Failed` warning (clause 6.3.17).
        Refused,
    }

    /// The history `member` is the one instance of, observed once.
    fn history(member: &str) -> Value {
        let mut instance: Value = serde_json::from_str(member).expect("the member is JSON");
        if let Some(members) = instance.as_object_mut() {
            members.insert("observedAt".to_owned(), json!("2020-01-01T00:00:00Z"));
        }
        json!([instance])
    }

    /// Assert what an answer carrying the history of `member` beside a valid
    /// sibling makes of it.
    async fn assert_verdict(member: &str, verdict: Verdict) {
        let body = json!([
            {"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "attr": history(member)},
            {"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "speed": [{"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z"}]}
        ]);
        let answer = answered(&body.to_string(), ContextSpec::core_only(), &ContextSpec::core_only()).await;
        match verdict {
            Verdict::Typed => {
                pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
                pretty_assertions::assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
            }
            Verdict::Refused => {
                pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:2"]);
                assert!(
                    matches!(answer.warnings.as_slice(), [warning] if warning.code() == WarnCode::RevalidationFailed),
                    "{:?}",
                    answer.warnings
                );
            }
        }
    }

    /// One test per shape, named after it: `member` written under the Core
    /// `@context`, with what this route makes of it.
    macro_rules! shape_tests {
        ($($(#[$doc:meta])* $test:ident: $member:literal => $verdict:ident),+ $(,)?) => {
            $(
                $(#[$doc])*
                #[tokio::test]
                async fn $test() {
                    assert_verdict($member, Verdict::$verdict).await;
                }
            )+
        };
    }

    shape_tests! {
        /// Clause 4.5.0 uses the NGSI-LD Null as the value of a deleted `GeoProperty`
        /// in a notification and a temporal representation; clause 5.5.4 refuses it
        /// elsewhere outside a merge.
        a_geoproperty_whose_value_is_the_null: r#"{"type": "GeoProperty", "value": "urn:ngsi-ld:null"}"# => Typed,
        /// Clause 4.5.2.2 requires the value member; clause 5.5.12 lets a merge keep
        /// the value held.
        an_instance_without_its_value: r#"{"type": "Property"}"# => Refused,
        /// Clause 4.4: `@type` stands for `type`.
        an_instance_spelled_with_keywords: r#"{"@type": "Property", "value": 1}"# => Typed,
    }
}
