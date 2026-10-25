//! A source answers in compact JSON-LD spelled in the forwarding context of the request. Each page is decoded element by element, an element the model cannot hold becoming a clause 6.3.17 warning instead of failing the page. Once every page is in, each entity is expanded under the context it was written in (its own `@context` when it names one, else the forwarding context the answer is spelled in), so answers merge by expanded name and are compacted once, under the client's context.
//!
//! Clauses 5.7.1.4 and 5.7.2.4: an entity whose `expiresAt` lies in the past is discarded, and so is every attribute instance whose own `expiresAt` has passed (clause 4.5.5.3), before anything is merged; an entity left with no attribute by that is discarded with it.

use crate::{
    FederationService,
    entity_reading::EntityReading,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedEntities, ForwardedResponse},
    source_answer_failure::SourceAnswerFailure,
    source_walk::{ReadPage, WalkedAnswer},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::operation::expiry_removal::InstanceExpiry;
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, received_state::ReceivedState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    received::{ContextualEntity, ReceivedElement, ReceivedEntities},
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

/// One source's answer of entities, decoded but not yet read into the model.
pub(crate) struct ReceivedAnswer {
    /// `None` for the entry that carries only the clause 6.3.17 loop warning.
    pub(crate) registration_id: Option<Identifier>,
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The context the answer is spelled in.
    pub(crate) naming: Box<ContextSpec>,
    /// The entities the source answered with, as it wrote them.
    pub(crate) entities: Vec<ContextualEntity>,
    /// The clause 6.3.17 warnings of the attempt.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The `EntityMap` the source used for the request (clause 5.5.14).
    pub(crate) linked_map: Option<Identifier>,
}

/// A Query Entities answer walks page by page as the entities each page
/// carried.
impl WalkedAnswer for ReceivedAnswer {
    type Element = ContextualEntity;

    fn read(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<ReceivedAnswer> {
        received_answer(registration, outcome)
    }

    fn elements(&mut self) -> &mut Vec<ContextualEntity> {
        &mut self.entities
    }

    fn warnings(&mut self) -> &mut Vec<NgsiLdWarning> {
        &mut self.warnings
    }

    fn identifier(element: &ContextualEntity) -> Option<Identifier> {
        element.id.clone() // clone: the walk remembers the identifier after the entity moves into the answer
    }

    fn absorb(&mut self, page: ReceivedAnswer) {
        self.warnings.extend(page.warnings);
    }
}

/// A query answers with an array and a retrieval with one object, but either shape is read from either: a retrieval continuing a clause 4.5.23 flat join answers with the entity and its linked entities side by side.
pub(crate) fn received_answer(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<ReceivedAnswer> {
    let answer = |naming, entities, warnings, linked_map| {
        Some(ReceivedAnswer {
            registration_id: Some(registration.id.clone()), // clone: the answer names the registration it came from
            mode: registration.access.mode.unwrap_or_default(),
            naming,
            entities,
            warnings,
            linked_map,
        })
    };
    match outcome {
        ForwardGetOutcome::Success(ForwardedResponse {
            body,
            naming,
            mut warnings,
            entity_map,
            ..
        }) => match ReceivedEntities::from_slice(&body) {
            Ok(ReceivedEntities(elements)) => {
                let sent = elements.len();
                let mut entities = Vec::with_capacity(sent);
                for element in elements {
                    match element {
                        ReceivedElement::Read(entity) => entities.push(entity),
                        ReceivedElement::Refused(refusal) => {
                            warn!("Context source {} returned an entity that is refused: {}", registration.id, refusal);
                            warnings.push(
                                SourceAnswerFailure::EntityRefused {
                                    registration: &registration.id,
                                    refusal,
                                }
                                .into_warning(),
                            );
                        }
                    }
                }
                ReadPage {
                    answer: answer(naming, entities, warnings, entity_map),
                    sent,
                }
            }
            Err(error) => {
                warn!(registration = %registration.id, %error, "context source answered with a body that is no entity document");
                warnings.push(
                    SourceAnswerFailure::EntityDocument {
                        registration: &registration.id,
                        error,
                    }
                    .into_warning(),
                );
                ReadPage::empty(answer(naming, Vec::new(), warnings, None))
            }
        },
        // A 404 is not abnormal (clause 6.3.17), but the warnings the source
        // reported about the sources behind it still travel.
        ForwardGetOutcome::NotFound(cs_warnings) => ReadPage::empty(
            (!cs_warnings.is_empty())
                .then(|| answer(Box::new(ContextSpec::none()), Vec::new(), cs_warnings, None))
                .flatten(),
        ),
        other @ (ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected) => {
            ReadPage::empty(other.warning(&registration.id).and_then(|warning| {
                warn!("Forwarding to context source {} failed: {}", registration.id, warning);
                answer(Box::new(ContextSpec::none()), Vec::new(), vec![warning], None)
            }))
        }
    }
}

impl FederationService {
    /// Read every answer of a forwarded read into the model.
    pub(crate) async fn answered_states(&self, outcome: ForwardReadOutcome<ReceivedAnswer>) -> ForwardReadOutcome<ForwardedEntities> {
        let ForwardReadOutcome { results, sources } = outcome;
        let now = Utc::now();
        let mut read = Vec::with_capacity(results.len());
        for answer in results {
            read.push(self.answered_entities(answer, now).await);
        }
        ForwardReadOutcome { results: read, sources }
    }

    /// Read one source's entities into the model at `now`, each refused
    /// entity a clause 6.3.17 warning naming it.
    async fn answered_entities(&self, answer: ReceivedAnswer, now: DateTime<Utc>) -> ForwardedEntities {
        let ReceivedAnswer {
            registration_id,
            mode,
            naming,
            entities,
            mut warnings,
            linked_map,
        } = answer;
        let source = registration_id.as_ref().map(ToString::to_string).unwrap_or_default();
        let mut states = Vec::with_capacity(entities.len());
        if !entities.is_empty() {
            match EntityExpansion::prepare(&self.engine, &naming).await {
                Ok(expansion) => {
                    for contextual in entities {
                        match self.received_state(&expansion, contextual).await {
                            Ok(received) => states.extend(unexpired(received, &source, now)),
                            Err(refusal) => {
                                warn!("Context source {} returned an entity that does not read: {}", source, refusal);
                                warnings.extend(registration_id.as_ref().map(|registration| {
                                    SourceAnswerFailure::EntityUnreadable {
                                        registration,
                                        reading: refusal,
                                    }
                                    .into_warning()
                                }));
                            }
                        }
                    }
                }
                Err(error) => {
                    warn!(
                        "The answer of context source {} names a context its entities cannot be read under: {}",
                        source, error
                    );
                    warnings.extend(
                        registration_id
                            .as_ref()
                            .map(|registration| SourceAnswerFailure::AnswerContext { registration, error }.into_warning()),
                    );
                }
            }
        }
        ForwardedEntities {
            registration_id,
            mode,
            entities: states,
            warnings,
            linked_map,
        }
    }

    /// Read one entity under its own `@context` when it names one (JSON-LD
    /// 1.1: a node's context applies to that node), else under the context
    /// the answer is spelled in, its members read as the core terms that
    /// context says they stand for.
    async fn received_state(&self, expansion: &EntityExpansion<'_>, mut contextual: ContextualEntity) -> Result<ReceivedState, EntityReading> {
        match contextual.context.take() {
            None => {
                let entity = contextual.entity(expansion.context().aliases()).map_err(EntityReading::Undecodable)?;
                expansion.received(entity).await.map_err(EntityReading::Refused)
            }
            Some(own) => {
                let own = EntityExpansion::prepare(&self.engine, &ContextSpec::from_body(own))
                    .await
                    .map_err(EntityReading::OwnContext)?;
                let entity = contextual.entity(own.context().aliases()).map_err(EntityReading::Undecodable)?;
                own.received(entity).await.map_err(EntityReading::Refused)
            }
        }
    }
}

/// The received entity, unless it expired at `now` (clauses 5.7.1.4, 5.7.2.4,
/// 4.5.5.3).
fn unexpired(mut received: ReceivedState, source: &str, now: DateTime<Utc>) -> Option<ReceivedState> {
    if received.state.expires_at().is_some_and(|expires_at| expires_at.get() <= now) {
        debug!(
            "Context source {} returned the expired entity {}, which is discarded",
            source,
            received.state.id()
        );
        return None;
    }
    let had_attributes = !received.state.attributes().is_empty();
    // Clause 5.7.2.4: an entity whose attribute data has all expired carries
    // nothing to merge and is discarded with it.
    if had_attributes && received.state.remove_expired_instances(now) == InstanceExpiry::Removed && received.state.attributes().is_empty() {
        debug!(
            "Context source {} returned the entity {} with only expired attributes, which is discarded",
            source,
            received.state.id()
        );
        return None;
    }
    Some(received)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        answer_reading::received_answer,
        fixed_registration_store::offline_service,
        outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedEntities, ForwardedResponse, MatchedSources},
        received_fixture::iri,
        source_continuation::SourceContinuation,
    };
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::json;
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

    /// What one source answering `body` under `naming` contributes, read into
    /// the model.
    async fn answered(body: &str, naming: ContextSpec) -> ForwardedEntities {
        let registration = registration();
        let outcome = ForwardGetOutcome::Success(ForwardedResponse {
            body: Arc::from(body.as_bytes()),
            naming: Box::new(naming),
            warnings: Vec::new(),
            entity_map: None,
            continuation: SourceContinuation::Complete,
            partial_range: None,
        });
        let page = received_answer(&registration, outcome);
        let service = offline_service(Vec::new());
        let outcome = ForwardReadOutcome {
            results: page.answer.into_iter().collect(),
            sources: MatchedSources::default(),
        };
        service.answered_states(outcome).await.results.pop().expect("one answer")
    }

    fn ids(answer: &ForwardedEntities) -> Vec<&str> {
        answer.entities.iter().map(|received| received.state.id().as_str()).collect()
    }

    /// An answer spelled under the core context reads every name expanded.
    #[tokio::test]
    async fn a_received_entity_is_expanded_under_its_source_context() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":50}}]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1"]);
        assert!(answer.entities[0].state.attributes().contains(&iri("speed")));
        assert_eq!(answer.warnings, [] as [NgsiLdWarning; 0]);
    }

    /// F-D6: an answer entity carrying its own `@context` is read under it,
    /// not under the context the answer is spelled in.
    #[tokio::test]
    async fn the_entity_context_wins_over_the_forwarding_context() {
        let answer = answered(
            r#"[{"@context":{"spd":"https://x.example/speed"},"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","spd":{"type":"Property","value":50}}]"#,
            ContextSpec::core_only(),
        )
        .await;
        assert!(
            answer.entities[0].state.attributes().contains("https://x.example/speed"),
            "{:?}",
            answer.entities[0].state
        );
    }

    /// The entity-level system times travel beside the state.
    #[tokio::test]
    async fn entity_system_times_are_kept() {
        let answer = answered(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","createdAt":"2024-01-01T00:00:00Z","modifiedAt":"2024-02-01T00:00:00Z"}"#,
            ContextSpec::core_only(),
        )
        .await;
        let times = answer.entities[0].system_times;
        assert!(times.created_at.is_some() && times.modified_at.is_some(), "{times:?}");
    }

    /// Clause 4.5.23: a Linked Entity a source nested under a Relationship is
    /// lifted out, the Relationship keeping its target.
    #[tokio::test]
    async fn nested_linked_entities_are_lifted() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","owner":{"type":"Relationship","object":"urn:ngsi-ld:Person:b","entity":{"id":"urn:ngsi-ld:Person:b","type":"Person","name":{"type":"Property","value":"Bo"}}}}]"#,
            ContextSpec::core_only(),
        )
        .await;
        let vehicle = &answer.entities[0];
        assert!(vehicle.state.attributes().contains(&iri("owner")));
        pretty_assertions::assert_eq!(
            vehicle.linked.iter().map(|linked| linked.state.id().as_str()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Person:b"]
        );
    }

    /// Decision F2: an entity the model cannot hold is refused with a clause
    /// 6.3.17 warning, the others kept.
    #[tokio::test]
    async fn an_invalid_remote_entity_is_refused_with_a_warning() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property"}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle"}]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:2"]);
        assert!(
            matches!(answer.warnings.as_slice(), [NgsiLdWarning::RevalidationFailed(_)]),
            "{:?}",
            answer.warnings
        );
    }

    /// A body that is no entity document is a warning, not a failure.
    #[tokio::test]
    async fn a_payload_that_is_not_json_is_a_warning() {
        let answer = answered("<html>", ContextSpec::core_only()).await;
        pretty_assertions::assert_eq!(answer.entities.len(), 0);
        assert!(
            matches!(answer.warnings.as_slice(), [NgsiLdWarning::RevalidationFailed(_)]),
            "{:?}",
            answer.warnings
        );
    }

    /// Clause 5.7.2.4: an expired entity is discarded, one still valid kept.
    #[tokio::test]
    async fn an_expired_forwarded_entity_is_discarded() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:old","type":"Vehicle","expiresAt":"2000-01-01T00:00:00Z"},{"id":"urn:ngsi-ld:Vehicle:new","type":"Vehicle","expiresAt":"2999-01-01T00:00:00Z"}]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:new"]);
    }

    /// Clause 5.7.2.4: an entity whose every attribute has expired is
    /// discarded whole; one with no attributes at all is not expired; an
    /// expired instance is dropped beside its siblings.
    #[tokio::test]
    async fn expired_attributes_are_discarded_before_the_merge() {
        let answer = answered(
            r#"[
                {"id":"urn:ngsi-ld:Vehicle:old","type":"Vehicle","speed":{"type":"Property","value":90,"expiresAt":"2000-01-01T00:00:00Z"}},
                {"id":"urn:ngsi-ld:Vehicle:bare","type":"Vehicle"},
                {"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":90,"expiresAt":"2000-01-01T00:00:00Z"},"fuelLevel":{"type":"Property","value":70,"expiresAt":"2999-01-01T00:00:00Z"}}
            ]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:bare", "urn:ngsi-ld:Vehicle:1"]);
        let kept = &answer.entities[1].state;
        assert!(!kept.attributes().contains(&iri("speed")));
        assert!(kept.attributes().contains(&iri("fuelLevel")));
    }

    /// Clauses 4.7 and 6.3.17: an entity whose geometry is off the WGS 84
    /// axes is refused with a warning, the source's other entities kept.
    #[tokio::test]
    async fn a_forwarded_entity_with_a_geometry_off_the_axes_is_refused() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:valid","type":"Vehicle","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[14.5,46.05]}}},{"id":"urn:ngsi-ld:Vehicle:invalid","type":"Vehicle","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[14.5,146.05]}}}]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:valid"]);
        pretty_assertions::assert_eq!(answer.warnings.len(), 1, "{:?}", answer.warnings);
    }

    /// Table 5.2.5-1 and clause 6.3.17: a `unitCode` a write would refuse is
    /// not relayed either, including one on a family that carries none.
    #[tokio::test]
    async fn a_forwarded_entity_with_an_unusable_unit_code_is_refused() {
        let answer = answered(
            r#"[
                {"id":"urn:ngsi-ld:Vehicle:kmh","type":"Vehicle","speed":{"type":"Property","value":50,"unitCode":"km/h"}},
                {"id":"urn:ngsi-ld:Vehicle:unitless","type":"Vehicle","parkedIn":{"type":"Relationship","object":"urn:ngsi-ld:Parking:1","unitCode":"KGM"}},
                {"id":"urn:ngsi-ld:Vehicle:valid","type":"Vehicle","speed":{"type":"Property","value":50,"unitCode":"KMH"}}
            ]"#,
            ContextSpec::core_only(),
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:valid"]);
        pretty_assertions::assert_eq!(answer.warnings.len(), 2, "{:?}", answer.warnings);
    }

    /// An exact decimal survives the reading.
    #[tokio::test]
    async fn a_forwarded_entity_keeps_its_exact_decimal_value() {
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}}]"#,
            ContextSpec::core_only(),
        )
        .await;
        let encoded = String::from_utf8(answer.entities[0].state.to_vec().expect("encodes")).expect("UTF-8");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
    }

    /// A context naming terms the answer is spelled in reads them expanded.
    #[tokio::test]
    async fn an_answer_is_read_under_the_context_it_is_spelled_in() {
        let naming = ContextSpec::from_body(JsonLiteral::admit(r#"{"spd":"https://x.example/speed"}"#).expect("a literal"));
        let answer = answered(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","spd":{"type":"Property","value":5}}]"#,
            naming,
        )
        .await;
        assert!(answer.entities[0].state.attributes().contains("https://x.example/speed"));
    }

    /// What a peer's answer to a query makes of every attribute member shape
    /// (clause 6.3.17): an entity read typed keeps its sibling company, one
    /// refused is dropped with one `111 Revalidation Failed` warning while its
    /// sibling is kept.
    mod shapes {
        use crate::answer_reading::tests::{answered, ids};
        use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
        use andromeda_json_literal::JsonLiteral;
        use andromeda_jsonld::context_spec::ContextSpec;

        /// What a route makes of a member written in one shape.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Verdict {
            /// The entity carrying the member is read and kept.
            Typed,
            /// The entity carrying the member is dropped with one `111 Revalidation
            /// Failed` warning (clause 6.3.17).
            Refused,
        }

        /// The user `@context` aliasing `@type`, `@id` and `ngsi-ld:hasValue`.
        const ALIASING_CONTEXT: &str = r#"{"kind": "@type", "ident": "@id", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}"#;

        /// Assert what an answer carrying `member`, written under the user
        /// `@context` `terms` or the Core `@context` alone, beside a valid sibling
        /// makes of the member.
        async fn assert_verdict(member: &str, terms: Option<&str>, verdict: Verdict) {
            let naming = terms.map_or_else(ContextSpec::core_only, |terms| {
                ContextSpec::from_body(JsonLiteral::admit(terms).expect("the aliasing context is JSON"))
            });
            let body = format!(r#"[{{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","attr":{member}}},{{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle"}}]"#);
            let answer = answered(&body, naming).await;
            match verdict {
                Verdict::Typed => {
                    pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
                    pretty_assertions::assert_eq!(answer.warnings, Vec::<NgsiLdWarning>::new());
                }
                Verdict::Refused => {
                    pretty_assertions::assert_eq!(ids(&answer), vec!["urn:ngsi-ld:Vehicle:2"]);
                    assert!(
                        matches!(answer.warnings.as_slice(), [NgsiLdWarning::RevalidationFailed(_)]),
                        "{:?}",
                        answer.warnings
                    );
                }
            }
        }

        /// One test per shape, named after it: `member` written under the optional
        /// user `@context`, with what this route makes of it.
        macro_rules! shape_tests {
            ($($(#[$doc:meta])* $test:ident: $member:literal $(under $context:ident)? => $verdict:ident),+ $(,)?) => {
                $(
                    $(#[$doc])*
                    #[tokio::test]
                    async fn $test() {
                        assert_verdict($member, [$($context)?].into_iter().next(), Verdict::$verdict).await;
                    }
                )+
            };
        }

        shape_tests! {
            /// Clause 4.5.2.3: a bare scalar is a concise Property.
            a_bare_scalar: "5" => Typed,
            /// Clause 4.5.2.3: an array of values is a concise Property.
            an_array_of_values: "[1, 2, 3]" => Typed,
            /// Clause 4.5.2.3: the empty array is a concise Property too.
            an_empty_array: "[]" => Typed,
            /// Clause 4.5.2.3: an object naming no type is a concise Property.
            a_typeless_object: r#"{"street": "Main", "no": 1}"# => Typed,
            /// Clause 4.5.2.3: a typeless instance carrying `value`.
            a_typeless_instance_with_a_value: r#"{"value": 21, "unitCode": "CEL"}"# => Typed,
            /// Clause 4.7.3: a typeless instance whose value is a geometry.
            a_typeless_instance_with_a_geometry_value: r#"{"value": {"type": "Point", "coordinates": [13.4, 52.5]}}"# => Typed,
            /// Clause 4.5.3.3: a typeless instance carrying `object`.
            a_typeless_instance_with_an_object: r#"{"object": "urn:ngsi-ld:Owner:1"}"# => Typed,
            /// Clause 4.5.18.3: a typeless instance carrying `languageMap`.
            a_typeless_instance_with_a_language_map: r#"{"languageMap": {"en": "Tower", "de": "Turm"}}"# => Typed,
            /// Clause 4.5.20.3: a typeless instance carrying `vocab`.
            a_typeless_instance_with_a_vocab: r#"{"vocab": "Tower"}"# => Typed,
            /// Clause 4.5.21.3: a typeless instance carrying `valueList`.
            a_typeless_instance_with_a_value_list: r#"{"valueList": [1, 2]}"# => Typed,
            /// Clause 4.5.22.3: a typeless instance carrying `objectList`.
            a_typeless_instance_with_an_object_list: r#"{"objectList": ["urn:ngsi-ld:Owner:1"]}"# => Typed,
            /// Clause 4.5.24.3: a typeless instance carrying `json`.
            a_typeless_instance_with_a_json_value: r#"{"json": {"a": [1, {"b": null}]}}"# => Typed,
            /// Clause 4.7.3: a bare geometry is a concise `GeoProperty`.
            a_bare_geometry: r#"{"type": "Point", "coordinates": [13.4, 52.5]}"# => Typed,
            /// Clause 4.5.5: a multi-attribute one of whose instances is concise.
            an_array_with_a_concise_instance: r#"[{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}]"# => Typed,
            /// Clause 4.7.2: a geometry may be written as the JSON string encoding it.
            a_geometry_written_as_a_string: r#"{"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}"# => Typed,
            /// Clause 4.7.2: a string that encodes no geometry is no `GeoProperty` value.
            a_string_encoding_no_geometry: r#"{"type": "GeoProperty", "value": "not a geometry"}"# => Refused,
            /// Clause 5.5.4 refuses the NGSI-LD Null outside a merge, which removes the
            /// member (clause 5.5.12), and a notification reports a deleted attribute so
            /// (clause 5.8.6).
            a_bare_ngsi_ld_null: r#""urn:ngsi-ld:null""# => Refused,
            /// Clause 4.5.0 uses the NGSI-LD Null as the value of a deleted `GeoProperty`
            /// in a notification and a temporal representation; clause 5.5.4 refuses it
            /// elsewhere outside a merge.
            a_geoproperty_whose_value_is_the_null: r#"{"type": "GeoProperty", "value": "urn:ngsi-ld:null"}"# => Refused,
            /// Clause 5.5.12: a merge removes the `unitCode` written as the NGSI-LD Null.
            a_null_unit_code: r#"{"type": "Property", "value": 1, "unitCode": "urn:ngsi-ld:null"}"# => Refused,
            /// Clause 5.5.12: a merge removes the `observedAt` written as the NGSI-LD Null.
            a_null_observed_at: r#"{"type": "Property", "value": 1, "observedAt": "urn:ngsi-ld:null"}"# => Refused,
            /// Clause 5.5.12, last bullet: a `datasetId` is never removed by a merge.
            a_null_dataset_id: r#"{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:null"}"# => Refused,
            /// Clauses 4.5.0 and 5.2.1: no member admits JSON `null`.
            a_json_null: "null" => Refused,
            /// Clause 4.5.2.2: a type naming no attribute type.
            an_unknown_type: r#"{"type": "Propertyy", "value": 1}"# => Refused,
            /// Clause 4.7.3: a geometry type without its coordinates.
            a_geometry_without_coordinates: r#"{"type": "Point"}"# => Refused,
            /// Clause 4.5.2.2 requires the value member; clause 5.5.12 lets a merge keep
            /// the value held.
            an_instance_without_its_value: r#"{"type": "Property"}"# => Refused,
            /// Clause 4.5.3.2: an `object` is a URI.
            an_undecodable_family_value: r#"{"type": "Relationship", "object": "not a uri"}"# => Refused,
            /// Clause 4.8: an `observedAt` is a `DateTime`.
            an_undecodable_envelope_member: r#"{"type": "Property", "value": 1, "observedAt": "yesterday"}"# => Refused,
            /// Clause 5.2.2: a producer's system member is ignored; a peer's own system
            /// members are `DateTime`s (clause 4.8).
            a_malformed_system_member: r#"{"type": "Property", "value": 1, "createdAt": "not a time"}"# => Refused,
            /// Clause 4.6.2: a sub-attribute is named by an attribute name.
            a_bad_sub_attribute_name: r#"{"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 2}}"# => Refused,
            /// Clause 4.5.5: a multi-attribute is an array of instances.
            a_mixed_array: r#"[{"type": "Property", "value": 1}, 1]"# => Refused,
            /// Clause 4.5.2.3: a concise instance infers one family.
            several_family_value_members: r#"{"object": "urn:ngsi-ld:Owner:1", "vocab": "Tower"}"# => Refused,
            /// Clause 5.5.7: no JSON-LD keyword inside a geometry.
            a_geometry_with_a_json_ld_keyword: r#"{"type": "GeoProperty", "value": {"@id": "urn:ngsi-ld:Geometry:1", "type": "Point", "coordinates": [13.4, 52.5]}}"# => Refused,
            /// Clause 4.4: `@type` stands for `type`.
            an_instance_spelled_with_keywords: r#"{"@type": "Property", "value": 1}"# => Typed,
            /// Clause 5.5.7: a user `@context` may alias `@type` and `ngsi-ld:hasValue`.
            an_instance_spelled_with_user_aliases: r#"{"kind": "Property", "v": 1}"# under ALIASING_CONTEXT => Typed,
        }
    }
}
