//! Reading each Temporal Evolution a source answered under the context it was written in (clauses 4.4, 5.5.4 and 5.5.7): its own `@context` when it names one (JSON-LD 1.1: a node object's own context applies to that node), else the forwarding context the answer is spelled in. The context may give a core term a term of its own, an alias of `@type` or of a temporal plural, and shape a member by the container of its term, so an evolution reads as the same evolution whichever spelling its context gives. Only an evolution invalid under that context is left out, with a clause 6.3.17 warning.

use crate::{
    FederationService,
    entity_reading::TemporalEntityReading,
    read_fan_out::SourceAnswer,
    received_temporal_answer::ReceivedTemporalAnswer,
    source_answer_failure::SourceAnswerFailure,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::expansion_context::ExpansionContext};
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_model::{received::ContextualEntity, temporal::temporal_body::TemporalBody};
use std::sync::Arc;
use tracing::warn;

/// One Temporal Evolution a source answered, read into the model.
#[derive(Debug)]
pub(crate) struct ReadTemporalEntity {
    /// The processed context the evolution was written in, which its names
    /// expand under.
    pub(crate) context: Arc<ExpansionContext>,
    /// The instances of its attributes, or the aggregates over them.
    pub(crate) body: TemporalBody,
}

impl FederationService {
    /// Read every evolution of each answer under the context it was written
    /// in.
    pub(crate) async fn read_temporal_answers(
        &self,
        answers: Vec<SourceAnswer<ReceivedTemporalAnswer<ContextualEntity>>>,
    ) -> Vec<SourceAnswer<ReceivedTemporalAnswer<ReadTemporalEntity>>> {
        let mut read = Vec::with_capacity(answers.len());
        for answer in answers {
            read.push(self.read_source_answer(answer).await);
        }
        read
    }

    /// Read every evolution of one source's answer under the context it was
    /// written in.
    pub(crate) async fn read_source_answer(
        &self,
        answer: SourceAnswer<ReceivedTemporalAnswer<ContextualEntity>>,
    ) -> SourceAnswer<ReceivedTemporalAnswer<ReadTemporalEntity>> {
        let SourceAnswer { mode, error_status, entry } = answer;
        let entry = match entry {
            Some(received) => Some(self.read_temporal_answer(received).await),
            None => None,
        };
        SourceAnswer { mode, error_status, entry }
    }

    /// Read every evolution of `answer`, each one that does not read a clause
    /// 6.3.17 warning naming it.
    pub(crate) async fn read_temporal_answer(&self, answer: ReceivedTemporalAnswer<ContextualEntity>) -> ReceivedTemporalAnswer<ReadTemporalEntity> {
        let ReceivedTemporalAnswer {
            registration_id,
            mode,
            naming,
            entities,
            mut warnings,
            linked_map,
            partial_ranges,
        } = answer;
        let source = registration_id.as_ref().map(ToString::to_string).unwrap_or_default();
        let spelled_in = if entities.iter().any(|contextual| contextual.context.is_none()) {
            match self.engine.expansion_context(&naming).await {
                Ok(context) => Some(context),
                Err(error) => {
                    warn!(
                        "The answer of context source {} names a context its temporal entities cannot be read under: {}",
                        source, error
                    );
                    warnings.extend(registration_id.as_ref().map(|registration| {
                        SourceAnswerFailure::AnswerContext {
                            registration,
                            error: ExpansionError::Context(error),
                        }
                        .into_warning()
                    }));
                    None
                }
            }
        } else {
            None
        };
        let mut read = Vec::with_capacity(entities.len());
        for contextual in entities {
            match self.read_evolution(contextual, spelled_in.as_ref()).await {
                Ok(Some(entity)) => read.push(entity),
                Ok(None) => {}
                Err(reading) => {
                    warn!("Context source {} returned a temporal entity that does not read: {}", source, reading);
                    warnings.extend(
                        registration_id
                            .as_ref()
                            .map(|registration| SourceAnswerFailure::TemporalEntityUnreadable { registration, reading }.into_warning()),
                    );
                }
            }
        }
        ReceivedTemporalAnswer {
            registration_id,
            mode,
            naming,
            entities: read,
            warnings,
            linked_map,
            partial_ranges,
        }
    }

    /// Read one evolution under its own `@context`, or under `spelled_in`,
    /// the forwarding context, when it names none: `None` for one naming no
    /// context of its own in an answer whose forwarding context does not
    /// process, which is reported once for the answer.
    async fn read_evolution(
        &self,
        mut contextual: ContextualEntity,
        spelled_in: Option<&Arc<ExpansionContext>>,
    ) -> Result<Option<ReadTemporalEntity>, TemporalEntityReading> {
        let context = match contextual.context.take() {
            Some(own) => self
                .engine
                .expansion_context(&ContextSpec::from_body(own))
                .await
                .map_err(|error| TemporalEntityReading::OwnContext(ExpansionError::Context(error)))?,
            None => match spelled_in {
                Some(context) => Arc::clone(context), // clone: Arc ref-count bump
                None => return Ok(None),
            },
        };
        let body = contextual.temporal(context.reading()).map_err(TemporalEntityReading::Undecodable)?;
        Ok(Some(ReadTemporalEntity { context, body }))
    }
}
