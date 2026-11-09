//! Each entity is expanded under the context it was read under (its own `@context` when it names one, else the forwarding context the answer is spelled in), so the histories of every holder merge by expanded name and are compacted once, under the client's context. The pages a source answered an entity in are put together here: a resumed walk answers the same entity again for the instants it did not reach, and an instance answered twice is the same instance, which the merge recognises by its `instanceId`.
//!
//! An aggregate carries no IRI to expand, so it is respelled instead: each name is expanded under the context the source spelled it in and compacted under the client's.

use crate::{
    FederationService,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    received_temporal_answer::ReceivedTemporalAnswer,
    source_answer_failure::SourceAnswerFailure,
    temporal_answer_reading::ReadTemporalEntity,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::{
    entity_type::EntityTypeValue,
    temporal::{aggregated_entity::AggregatedTemporalEntity, temporal_body::TemporalBody, temporal_document::MergeMode, temporal_history::TemporalHistory},
};
use indexmap::{IndexMap, map::Entry};
use tracing::warn;

impl FederationService {
    /// `context` is the client's, which an aggregate is respelled in, and `timeproperty` the Temporal Property the instances of one entity are ordered by when the pages of its history are put together.
    pub(crate) async fn answered_temporal(
        &self,
        outcome: ForwardReadOutcome<ReceivedTemporalAnswer<ReadTemporalEntity>>,
        context: &ContextSpec,
        timeproperty: TimeProp,
    ) -> ForwardReadOutcome<ForwardedTemporalEntities> {
        let ForwardReadOutcome { results, sources } = outcome;
        let mut read = Vec::with_capacity(results.len());
        for answer in results {
            read.push(self.answered_histories(answer, context, timeproperty).await);
        }
        ForwardReadOutcome { results: read, sources }
    }

    /// Read one source's entities into the model, the entities left out on
    /// any page, in any walk or here reported with one clause 6.3.17 warning.
    async fn answered_histories(
        &self,
        answer: ReceivedTemporalAnswer<ReadTemporalEntity>,
        context: &ContextSpec,
        timeproperty: TimeProp,
    ) -> ForwardedTemporalEntities {
        let ReceivedTemporalAnswer {
            registration_id,
            mode,
            naming,
            entities,
            mut warnings,
            mut dropped,
            linked_map,
            partial_ranges,
        } = answer;
        let source = registration_id.as_ref().map_or_default(ToString::to_string);
        let mut gathered: IndexMap<Identifier, TemporalHistory> = IndexMap::with_capacity(entities.len());
        let mut client_names: Option<NameCompaction> = None;
        for ReadTemporalEntity { id, context: written_in, body } in entities {
            let history = match body {
                TemporalBody::Evolution(entity) => match EntityExpansion::from_context(&self.engine, written_in).temporal(entity).await {
                    Ok(document) => TemporalHistory::Instances(document),
                    Err(refusal) => {
                        warn!("Context source {} returned a temporal entity that does not read: {}", source, refusal);
                        dropped.record(id);
                        continue;
                    }
                },
                TemporalBody::Aggregated(entity) => {
                    if client_names.is_none() {
                        match self.engine.compactor(context).processed_context().await {
                            Ok(processed) => client_names = Some(NameCompaction::new(processed)),
                            Err(error) => {
                                warnings.extend(
                                    registration_id
                                        .as_ref()
                                        .map(|registration| SourceAnswerFailure::AggregateContext { registration, error }.into_warning()),
                                );
                                continue;
                            }
                        }
                    }
                    let Some(names) = &client_names else {
                        continue;
                    };
                    TemporalHistory::Aggregates(self.respelled_aggregates(entity, &naming, names).await)
                }
            };
            match history.id().cloned().map(|id| gathered.entry(id)) {
                Some(Entry::Occupied(mut held)) => held.get_mut().merge(history, MergeMode::Inclusive, timeproperty),
                Some(Entry::Vacant(vacant)) => {
                    vacant.insert(history);
                }
                None => {}
            }
        }
        warnings.extend(registration_id.as_ref().and_then(|registration| dropped.warning(registration)));
        ForwardedTemporalEntities {
            registration_id,
            mode,
            entities: gathered.into_values().collect(),
            warnings,
            linked_map,
            partial_ranges,
        }
    }

    /// A name that does not expand stays as the source spelled it.
    async fn respelled_aggregates(&self, entity: AggregatedTemporalEntity, naming: &ContextSpec, names: &NameCompaction) -> AggregatedTemporalEntity {
        let AggregatedTemporalEntity { id, type_, attrs } = entity;
        let type_ = match type_ {
            EntityTypeValue::Single(name) => EntityTypeValue::Single(self.respelled_type(&name, naming, names).await.unwrap_or(name)),
            EntityTypeValue::Multiple(types) => {
                let mut respelled = Vec::with_capacity(types.len());
                for name in types {
                    respelled.push(self.respelled_type(&name, naming, names).await.unwrap_or(name));
                }
                EntityTypeValue::Multiple(respelled)
            }
        };
        let mut respelled_attrs = IndexMap::with_capacity(attrs.len());
        for (name, instances) in attrs {
            let name = self.respelled_attribute(&name, naming, names).await.unwrap_or(name);
            respelled_attrs.insert(name, instances);
        }
        AggregatedTemporalEntity {
            id,
            type_,
            attrs: respelled_attrs,
        }
    }
}

#[cfg(test)]
mod tests;
