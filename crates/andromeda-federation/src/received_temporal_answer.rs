//! A source answers in compact JSON-LD spelled in the forwarding context of the request. Each page is decoded element by element straight from its text, an element whose text a request's would be refused for becoming a clause 6.3.17 warning instead of failing the page. Each element is held as written until the context it was written in was processed, then read as a Temporal Evolution under it.

use crate::{
    FederationService,
    dropped_entities::DroppedEntities,
    outcome::{ForwardGetOutcome, ForwardedResponse},
    source_answer_failure::SourceAnswerFailure,
    source_walk::{ReadPage, WalkedAnswer},
    temporal_answer_reading::ReadTemporalEntity,
};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    received::{ContextualEntity, ReceivedElement, ReceivedEntities},
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
    temporal::temporal_body::TemporalBody,
};
use tracing::warn;

/// One source's answer of Temporal Evolutions: each entity held as written
/// (`E` is [`ContextualEntity`]) while the answer is walked, then read under
/// the context it was written in (`E` is [`ReadTemporalEntity`]).
pub(crate) struct ReceivedTemporalAnswer<E> {
    /// `None` for the entry that carries only the clause 6.3.17 loop warning.
    pub(crate) registration_id: Option<Identifier>,
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The context the answer is spelled in.
    pub(crate) naming: Box<ContextSpec>,
    /// The entities the source answered with.
    pub(crate) entities: Vec<E>,
    /// The clause 6.3.17 warnings of the attempt.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The entities left out of the answer so far, reported with one warning once the answer is read.
    pub(crate) dropped: DroppedEntities,
    /// The `EntityMap` the source used for the request (clause 5.5.14).
    pub(crate) linked_map: Option<Identifier>,
    /// The `Content-Range` of every page of the answer that the source cut
    /// short (clause 6.3.10): the instances past each range are still to come
    /// from the source, so the merged history is complete only up to them.
    pub(crate) partial_ranges: Vec<NgsiLdContentRange>,
}

impl ReceivedTemporalAnswer<ReadTemporalEntity> {
    /// How many attribute instances the answer carries.
    pub(crate) fn instance_count(&self) -> usize {
        self.entities.iter().map(|read| body_instance_count(&read.body)).sum()
    }
}

/// How many attribute instances the Temporal Evolution `body` carries; an
/// aggregate carries none.
pub(crate) fn body_instance_count(body: &TemporalBody) -> usize {
    match body {
        TemporalBody::Evolution(entity) => entity.attribute_members().map(|(_, attribute)| attribute.instances().len()).sum(),
        TemporalBody::Aggregated(_) => 0,
    }
}

/// A Query Temporal Evolution of Entities answer walks page by page as the
/// temporal entities each page carried.
impl WalkedAnswer for ReceivedTemporalAnswer<ContextualEntity> {
    type Element = ContextualEntity;

    fn read(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<ReceivedTemporalAnswer<ContextualEntity>> {
        FederationService::received_temporal_answer(registration, outcome)
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

    fn absorb(&mut self, page: ReceivedTemporalAnswer<ContextualEntity>) {
        self.warnings.extend(page.warnings);
        self.dropped.absorb(page.dropped);
        self.partial_ranges.extend(page.partial_ranges);
    }
}

impl FederationService {
    /// Read a forwarded temporal GET, with the clause 6.3.17 warning for every
    /// abnormal outcome. A 404 contributes nothing. A query answers with an
    /// array and a retrieval with one object, but either is read from either:
    /// a retrieval continuing a clause 4.5.23 flat join answers with an array.
    /// An element whose text is refused is dropped, and counted in the one
    /// warning the source's dropped entities are reported with.
    pub(crate) fn received_temporal_answer(
        registration: &ContextSourceRegistration,
        outcome: ForwardGetOutcome,
    ) -> ReadPage<ReceivedTemporalAnswer<ContextualEntity>> {
        let answered = |naming, entities, warnings, dropped, linked_map, partial_range: Option<NgsiLdContentRange>| {
            Some(ReceivedTemporalAnswer {
                registration_id: Some(registration.id.clone()),
                mode: registration.access.mode.unwrap_or_default(),
                naming,
                entities,
                warnings,
                dropped,
                linked_map,
                partial_ranges: partial_range.into_iter().collect(),
            })
        };
        match outcome {
            ForwardGetOutcome::Success(ForwardedResponse {
                body,
                naming,
                mut warnings,
                entity_map,
                partial_range,
                ..
            }) => match ReceivedEntities::from_slice(&body) {
                Ok(ReceivedEntities(elements)) => {
                    let sent = elements.len();
                    let mut entities = Vec::with_capacity(sent);
                    let mut dropped = DroppedEntities::default();
                    for element in elements {
                        match element {
                            ReceivedElement::Read(entity) => entities.push(entity),
                            ReceivedElement::Refused(refusal) => {
                                warn!("Context source {} returned a temporal entity that is refused: {}", registration.id, refusal);
                                dropped.record(refusal.id);
                            }
                        }
                    }
                    ReadPage {
                        answer: answered(naming, entities, warnings, dropped, entity_map, partial_range),
                        sent,
                    }
                }
                Err(error) => {
                    warn!(registration = %registration.id, %error, "context source answered with a body that is no temporal entity document");
                    warnings.push(
                        SourceAnswerFailure::TemporalDocument {
                            registration: &registration.id,
                            error,
                        }
                        .into_warning(),
                    );
                    ReadPage::empty(answered(naming, Vec::new(), warnings, DroppedEntities::default(), None, None))
                }
            },
            // A 404 is not abnormal (clause 6.3.17), but the warnings the
            // source reported about the sources behind it still travel.
            ForwardGetOutcome::NotFound(cs_warnings) => ReadPage::empty(
                (!cs_warnings.is_empty())
                    .then(|| answered(Box::new(ContextSpec::none()), Vec::new(), cs_warnings, DroppedEntities::default(), None, None))
                    .flatten(),
            ),
            other @ (ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected) => {
                ReadPage::empty(other.warning(&registration.id).and_then(|warning| {
                    warn!("Temporal forwarding to context source {} failed: {}", registration.id, warning);
                    answered(Box::new(ContextSpec::none()), Vec::new(), vec![warning], DroppedEntities::default(), None, None)
                }))
            }
        }
    }
}
