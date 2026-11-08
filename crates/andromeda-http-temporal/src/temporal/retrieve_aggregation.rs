//! Aggregating at this broker the entity of a temporal retrieval that is held in more than one part, when the aggregated representation is asked for (clauses 4.5.19 and 5.7.3.4).
//!
//! Clause 5.7.3.4 merges each source's answer into S1 "according to the algorithm defined in clause 4.5.5" and builds the representation last, from "the set of Attribute Instances that are in S1": the aggregates are of the merged instances. A holder's aggregate of its own part is the entity's only while no other part exists, so an entity held in more than one part is aggregated here over the instances of every part, this broker's own included. The deployment's [`AggregationGathering`] decides how the instances are asked for: in the one forwarded round, or in a second round once the first showed more than one part.

use crate::temporal::{
    ProblemResponse,
    TemporalQueryParameters,
    collect_temporal_warnings,
    merge_temporal_retrieve_results,
    query::{forwarding::forwarding_outcome, hub_aggregation::render_merged_instances},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    FederationService,
    aggregation_gathering::AggregationGathering,
    instance_budget::InstanceBudget,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    query_forwarding_error::QueryForwardingError,
    request::ForwardRetrieveRequest,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::window::TemporalWindow;
use andromeda_temporal::{
    merged_parts::{EntityParts, LocalPart},
    query_reading::QueryReading,
    service::TemporalService,
};
use axum::http::Uri;

/// How a federated temporal retrieval reads its sources and puts their parts together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetrievalAggregation {
    /// The representation asked for is not aggregated: the sources answer in it and their answers are merged with the local one.
    AsAnswered,
    /// The aggregated representation is asked for: an entity held in more than one part is aggregated here, its instances gathered as the gathering says.
    AtHub(AggregationGathering),
}

impl RetrievalAggregation {
    /// How the retrieval of `params` reads its sources, an aggregated one gathering the instances as `gathering` says.
    pub(crate) fn of(params: &TemporalQueryParameters, gathering: AggregationGathering) -> RetrievalAggregation {
        if params.parse_format().is_ok_and(|format| format.is_aggregated()) {
            RetrievalAggregation::AtHub(gathering)
        } else {
            RetrievalAggregation::AsAnswered
        }
    }
}

/// How a temporal retrieval reaches its sources, and what aggregating its entity here reads of it.
#[derive(Clone, Copy)]
pub(crate) struct RetrievalRound<'a> {
    /// The service the local part is read and aggregated in.
    pub(crate) service: &'a TemporalService,
    /// The service that contacts every matching registration.
    pub(crate) forwarding: &'a FederationService,
    /// The branch the retrieval reads.
    pub(crate) target: &'a ContextTarget,
    /// The forwarded retrieval, naming the entity and the request context.
    pub(crate) request: ForwardRetrieveRequest<'a>,
    /// The retrieval's parameters.
    pub(crate) params: &'a TemporalQueryParameters,
    /// The request URI a failure names.
    pub(crate) instance: &'a Uri,
}

impl RetrievalRound<'_> {
    /// Ask every matching source for its part of the history: the instances of the window when `aggregation` gathers every instance in this round, the representation the client asked for otherwise.
    ///
    /// # Errors
    ///
    /// The clause 4.12 `TooManyResults` refusal when the sources answer with more instances than one request gathers, and the classified failure of a forwarding this broker's own store failed.
    pub(crate) async fn first(self, aggregation: RetrievalAggregation) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, ProblemResponse> {
        match aggregation {
            RetrievalAggregation::AtHub(AggregationGathering::EveryInstance) => self.instances().await,
            RetrievalAggregation::AsAnswered | RetrievalAggregation::AtHub(AggregationGathering::OverlapsAgain) => {
                let forwarded = self
                    .forwarding
                    .forward_temporal_retrieve(self.target, self.request)
                    .await
                    .map_err(QueryForwardingError::Federation);
                forwarding_outcome(forwarded, self.instance)
            }
        }
    }

    /// Ask every matching source for the instances of the window, each answer read to the end of it.
    ///
    /// # Errors
    ///
    /// As for [`RetrievalRound::first`].
    async fn instances(self) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, ProblemResponse> {
        let budget = InstanceBudget::new(self.forwarding.max_query_instances());
        let forwarded = self.forwarding.forward_temporal_retrieve_history(self.target, self.request, &budget).await;
        forwarding_outcome(forwarded, self.instance)
    }

    /// The entity's history put together from `local`, its local part, and `answered`, what the sources answered the first round with, each part narrowed to `window`; `None` when no part holds it. The parts are aggregated here once more than one holds the entity and the aggregated representation is asked for, and merged as answered otherwise. Beside the history come the clause 6.3.17 warnings of a second round and of the parts that could not be aggregated.
    ///
    /// # Errors
    ///
    /// The failures of a second round, and the classified failure of compiling the retrieval or of an aggregation that exceeds its bound.
    pub(crate) async fn put_together(
        self,
        aggregation: RetrievalAggregation,
        local: Option<TemporalHistory>,
        answered: Vec<ForwardedTemporalEntities>,
        window: TemporalWindow,
    ) -> Result<(Option<TemporalHistory>, Vec<NgsiLdWarning>), ProblemResponse> {
        let local_part = if local.is_some() { LocalPart::Included } else { LocalPart::Excluded };
        let (parts, mut warnings) = self.gather_parts(aggregation, local_part, answered).await?;
        let history = match parts {
            RetrievedParts::Merged(answered) => merge_temporal_retrieve_results(local, answered, window),
            RetrievedParts::AggregatedHere(answered) => {
                let (aggregated, aggregation_warnings) = self.aggregate_here(local_part, answered).await?;
                warnings.extend(aggregation_warnings);
                // A local part holding nothing in the window still answers, as the merge would have it.
                aggregated.or(local)
            }
        };
        Ok((history, warnings))
    }

    /// How the parts of the retrieved entity are put together, given what the sources answered the first round with. Under [`AggregationGathering::OverlapsAgain`] the instances are asked in a second round, whose clause 6.3.17 warnings come back beside the parts.
    ///
    /// # Errors
    ///
    /// The failures of the second round.
    async fn gather_parts(
        self,
        aggregation: RetrievalAggregation,
        local: LocalPart,
        answered: Vec<ForwardedTemporalEntities>,
    ) -> Result<(RetrievedParts, Vec<NgsiLdWarning>), ProblemResponse> {
        let answering = answered.iter().filter(|source| !source.entities.is_empty()).count();
        let held_parts = answering + usize::from(local == LocalPart::Included);
        match aggregation {
            RetrievalAggregation::AtHub(AggregationGathering::EveryInstance) if answering > 0 => {
                Ok((RetrievedParts::AggregatedHere(instance_documents(answered, self.request.entity_id)), Vec::new()))
            }
            RetrievalAggregation::AtHub(AggregationGathering::OverlapsAgain) if held_parts > 1 => {
                let regathered = self.instances().await?;
                let warnings = collect_temporal_warnings(&regathered.results);
                let parts = instance_documents(regathered.results, self.request.entity_id);
                if parts.is_empty() {
                    return Ok((RetrievedParts::Merged(answered), warnings));
                }
                Ok((RetrievedParts::AggregatedHere(parts), warnings))
            }
            RetrievalAggregation::AsAnswered | RetrievalAggregation::AtHub(AggregationGathering::EveryInstance | AggregationGathering::OverlapsAgain) => {
                Ok((RetrievedParts::Merged(answered), Vec::new()))
            }
        }
    }

    /// The entity aggregated over `answered` and, when `local` includes it, its local instances; `None` when no part holds an instance of the window.
    ///
    /// # Errors
    ///
    /// The classified failure of compiling the retrieval or of an aggregation that exceeds its bound.
    async fn aggregate_here(self, local: LocalPart, answered: Vec<TemporalDocument>) -> Result<(Option<TemporalHistory>, Vec<NgsiLdWarning>), ProblemResponse> {
        let reading = QueryReading {
            context: self.request.context,
            language_targeting: self.service.language_targeting(),
        };
        let parts = vec![EntityParts {
            entity_id: self.request.entity_id.clone(),
            local,
            answered,
        }];
        let (entities, warnings) = render_merged_instances(self.service, self.target, self.params, reading, parts, self.instance).await?;
        Ok((entities.into_iter().next(), warnings))
    }
}

/// What the parts of a retrieved entity are put together as.
enum RetrievedParts {
    /// The answers, merged with the local history as they are.
    Merged(Vec<ForwardedTemporalEntities>),
    /// The instances every source answered, aggregated here with the local ones.
    AggregatedHere(Vec<TemporalDocument>),
}

/// Every document of `entity_id` that `sources` answered with instances, taken from them; an aggregate holds no
/// instance, and a document of another entity is no part of the one retrieved, so both are left out.
fn instance_documents(sources: Vec<ForwardedTemporalEntities>, entity_id: &Identifier) -> Vec<TemporalDocument> {
    sources
        .into_iter()
        .flat_map(|source| source.entities)
        .filter_map(|entity| match entity {
            TemporalHistory::Instances(document) => Some(document),
            TemporalHistory::Aggregates(_) => None,
        })
        .filter(|document| document.id() == Some(entity_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal::retrieve_aggregation::{RetrievalAggregation, instance_documents},
        test_history::held_history,
    };
    use andromeda_federation::{aggregation_gathering::AggregationGathering, outcome::ForwardedTemporalEntities};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat};
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
    use serde_json::json;

    fn asking(format: TemporalFormat) -> TemporalQueryParameters {
        TemporalQueryParameters {
            format: Some(WrittenValue::Supported(format)),
            ..TemporalQueryParameters::default()
        }
    }

    /// Clause 5.7.3.4: only an aggregated representation is built from the merged instances, so only it is aggregated here, gathered as the deployment says.
    #[test]
    fn only_an_aggregated_retrieval_is_aggregated_at_the_hub() {
        for gathering in [AggregationGathering::EveryInstance, AggregationGathering::OverlapsAgain] {
            pretty_assertions::assert_eq!(
                RetrievalAggregation::of(&asking(TemporalFormat::AggregatedValues), gathering),
                RetrievalAggregation::AtHub(gathering)
            );
            for format in [TemporalFormat::Normalized, TemporalFormat::TemporalValues] {
                pretty_assertions::assert_eq!(RetrievalAggregation::of(&asking(format), gathering), RetrievalAggregation::AsAnswered);
            }
            pretty_assertions::assert_eq!(
                RetrievalAggregation::of(&TemporalQueryParameters::default(), gathering),
                RetrievalAggregation::AsAnswered
            );
        }
    }

    /// A source may answer an entity other than the one retrieved; only the retrieved entity's instances are
    /// aggregated.
    #[tokio::test]
    async fn only_the_retrieved_entitys_instances_are_aggregated() {
        let instances = |id: &str| json!({"id": id, "type": "Vehicle", "speed": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]});
        let sources = vec![ForwardedTemporalEntities {
            registration_id: None,
            mode: ForwardingMode::Inclusive,
            entities: vec![
                held_history(instances("urn:ngsi-ld:Vehicle:1")).await,
                held_history(instances("urn:ngsi-ld:Vehicle:2")).await,
            ],
            warnings: Vec::new(),
            linked_map: None,
            partial_ranges: Vec::new(),
        }];
        let retrieved: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier");
        let taken: Vec<String> = instance_documents(sources, &retrieved)
            .iter()
            .filter_map(|document| document.id().map(ToString::to_string))
            .collect();
        pretty_assertions::assert_eq!(taken, vec![String::from("urn:ngsi-ld:Vehicle:1")]);
    }
}
