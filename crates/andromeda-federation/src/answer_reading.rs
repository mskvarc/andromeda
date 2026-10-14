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
        Some(element.entity.id.clone()) // clone: the walk remembers the identifier after the entity moves into the answer
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
    /// the answer is spelled in.
    async fn received_state(&self, expansion: &EntityExpansion<'_>, contextual: ContextualEntity) -> Result<ReceivedState, EntityReading> {
        let ContextualEntity { context, entity } = contextual;
        match context {
            None => expansion.received(entity).await.map_err(EntityReading::Refused),
            Some(own) => {
                let own = EntityExpansion::prepare(&self.engine, &ContextSpec::from_body(own))
                    .await
                    .map_err(EntityReading::OwnContext)?;
                own.received(entity).await.map_err(EntityReading::Refused)
            }
        }
    }
}

/// The received entity, unless it expired at `now` (clauses 5.7.1.4, 5.7.2.4,
/// 4.5.5.3).
fn unexpired(mut received: ReceivedState, source: &str, now: DateTime<Utc>) -> Option<ReceivedState> {
    if received.state.expires_at().is_some_and(|expires_at| expires_at.instant() <= now) {
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
}
