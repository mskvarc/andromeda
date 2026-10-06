//! Reading a context source's answer of Temporal Evolutions into the model
//! (clauses 5.7.3.4, 5.7.4.4 and 6.3.17).
//!
//! A source answers in compact JSON-LD spelled in the forwarding context of
//! the request. Each page is decoded element by element straight from its
//! text, an element the model cannot hold becoming a clause 6.3.17 warning
//! instead of failing the page. Once every page is in, each entity is expanded
//! under the context it was written in, so the histories of every holder merge
//! by expanded name (`crate::answered_temporal`).

use crate::{
    FederationService,
    outcome::{ForwardGetOutcome, ForwardedResponse},
    source_walk::{ReadPage, WalkedAnswer},
};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    received::ReceivedElement,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
    temporal::{
        received_temporal::{ContextualTemporalEntity, ReceivedTemporalEntities},
        temporal_body::TemporalBody,
    },
};
use tracing::warn;

/// One source's answer of Temporal Evolutions, decoded but not yet read into
/// the model.
pub(crate) struct ReceivedTemporalAnswer {
    /// The registration the entities came from; `None` for the entry that
    /// carries only the clause 6.3.17 loop warning.
    pub(crate) registration_id: Option<Identifier>,
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The context the answer is spelled in.
    pub(crate) naming: Box<ContextSpec>,
    /// The entities the source answered with, as it wrote them.
    pub(crate) entities: Vec<ContextualTemporalEntity>,
    /// The clause 6.3.17 warnings of the attempt.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The `EntityMap` the source used for the request (clause 5.5.14).
    pub(crate) linked_map: Option<Identifier>,
    /// The `Content-Range` of every page of the answer that the source cut
    /// short (clause 6.3.10): the instances past each range are still to come
    /// from the source, so the merged history is complete only up to them.
    pub(crate) partial_ranges: Vec<NgsiLdContentRange>,
}

impl ReceivedTemporalAnswer {
    /// How many attribute instances the answer carries.
    pub(crate) fn instance_count(&self) -> usize {
        self.entities.iter().map(|contextual| body_instance_count(&contextual.body)).sum()
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
impl WalkedAnswer for ReceivedTemporalAnswer {
    type Element = ContextualTemporalEntity;

    fn read(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<ReceivedTemporalAnswer> {
        FederationService::received_temporal_answer(registration, outcome)
    }

    fn elements(&mut self) -> &mut Vec<ContextualTemporalEntity> {
        &mut self.entities
    }

    fn warnings(&mut self) -> &mut Vec<NgsiLdWarning> {
        &mut self.warnings
    }

    fn identifier(element: &ContextualTemporalEntity) -> Option<Identifier> {
        Some(element.body.id().clone()) // clone: the walk remembers the identifier after the entity moves into the answer
    }

    fn absorb(&mut self, page: ReceivedTemporalAnswer) {
        self.warnings.extend(page.warnings);
        self.partial_ranges.extend(page.partial_ranges);
    }
}

impl FederationService {
    /// Read a forwarded temporal GET, with the clause 6.3.17 warning for every
    /// abnormal outcome. A 404 contributes nothing. A query answers with an
    /// array and a retrieval with one object, but either is read from either:
    /// a retrieval continuing a clause 4.5.23 flat join answers with an array.
    /// An entity the model cannot hold is dropped with a warning of its own.
    pub(crate) fn received_temporal_answer(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<ReceivedTemporalAnswer> {
        let answered = |naming, entities, warnings, linked_map, partial_range: Option<NgsiLdContentRange>| {
            Some(ReceivedTemporalAnswer {
                registration_id: Some(registration.id.clone()), // clone: the answer names the registration it came from
                mode: registration.access.mode.unwrap_or_default(),
                naming,
                entities,
                warnings,
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
            }) => match ReceivedTemporalEntities::from_slice(&body) {
                Ok(ReceivedTemporalEntities(elements)) => {
                    let sent = elements.len();
                    let mut entities = Vec::with_capacity(sent);
                    for element in elements {
                        match element {
                            ReceivedElement::Read(entity) => entities.push(entity),
                            ReceivedElement::Refused(refusal) => {
                                warn!("Context source {} returned a temporal entity that is refused: {}", registration.id, refusal);
                                warnings.push(NgsiLdWarning::RevalidationFailed(format!(
                                    "Context source {} returned a temporal entity that is refused: {refusal}",
                                    registration.id
                                )));
                            }
                        }
                    }
                    ReadPage {
                        answer: answered(naming, entities, warnings, entity_map, partial_range),
                        sent,
                    }
                }
                Err(error) => {
                    warn!(registration = %registration.id, %error, "context source answered with a body that is no temporal entity document");
                    warnings.push(NgsiLdWarning::RevalidationFailed(format!(
                        "Context source {} returned a payload that is no temporal entity document: {error}",
                        registration.id
                    )));
                    ReadPage::empty(answered(naming, Vec::new(), warnings, None, None))
                }
            },
            // A 404 is not abnormal (clause 6.3.17), but the warnings the
            // source reported about the sources behind it still travel.
            ForwardGetOutcome::NotFound(cs_warnings) => ReadPage::empty(
                (!cs_warnings.is_empty())
                    .then(|| answered(Box::new(ContextSpec::none()), Vec::new(), cs_warnings, None, None))
                    .flatten(),
            ),
            other @ (ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected) => {
                ReadPage::empty(other.warning(&registration.id).and_then(|warning| {
                    warn!("Temporal forwarding to context source {} failed: {}", registration.id, warning);
                    answered(Box::new(ContextSpec::none()), Vec::new(), vec![warning], None, None)
                }))
            }
        }
    }
}
