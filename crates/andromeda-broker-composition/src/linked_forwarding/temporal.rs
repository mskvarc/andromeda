//! The Temporal API half of [`LinkedForwarding`]: a Linked Entity's temporal
//! evolution is fetched as a clause 5.7.3 retrieval over the requester's
//! window, and the partial histories several sources hold of one entity are
//! merged.
//!
//! The forwarded retrieval carries the requesting client's own window, so the
//! roots and their Linked Entities describe one consistent slice of history
//! rather than being spliced together from two different ones.
//!
//! The temporal join is an Andromeda extension (clauses 5.7.3.4 and 5.7.4.4
//! refuse Linked Entity retrieval on the temporal operations), so a source is
//! never sent a temporal `join` it has not declared it serves. The one place a
//! source declares its configuration is the `contextSourceExtras` of its
//! identity (clause 5.15.1, Table 5.2.40-1): a holder resolves its own
//! sub-graph only when every source the retrieval reaches advertises the
//! extension there, and is asked for the entity alone otherwise.

use crate::linked_forwarding::{LinkedForwarding, linked_request, resolution, traversal_parameters::traversal_continuation};
use andromeda_extensions::{advertisement::advertises, enabled::Extension};
use andromeda_federation::{
    forwarded_query::client_query::{ClientQuery, TemporalTraversal},
    request::ForwardRetrieveRequest,
    source_identity::SourceConfiguration,
    source_reach::SourceReach,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::source::{RemoteReach, RemoteResolution};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::MergeMode, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{
    error::TemporalError,
    remote_linked::{HeldPart, RemoteTemporalLinkedSource, TemporalLinkedWindow},
};
use async_trait::async_trait;
use std::num::NonZeroU32;
use tracing::warn;

/// The parameters of the forwarded temporal retrieval: the window the answer
/// must reproduce, so the roots and their Linked Entities describe one
/// consistent slice of history, plus the traversal continuation.
fn linked_temporal_retrieval_parameters(depth: NonZeroU32, visited: &[Identifier], window: &TemporalLinkedWindow) -> TemporalQueryParameters {
    let (join, join_level, contained_by) = traversal_continuation(depth, visited);
    TemporalQueryParameters {
        join,
        join_level,
        contained_by,
        ..window.to_parameters()
    }
}

/// How far the sources described by `configurations` resolve on their own:
/// the whole remaining depth only when there is at least one and every one
/// advertises the temporal Linked Entity retrieval extension.
fn reach_of(configurations: &[SourceConfiguration]) -> RemoteReach {
    let advertising = |configuration: &SourceConfiguration| match configuration {
        SourceConfiguration::Described(extras) => advertises(extras.as_ref(), Extension::TemporalLinkedRetrieval),
        SourceConfiguration::Undescribed => false,
    };
    if !configurations.is_empty() && configurations.iter().all(advertising) {
        RemoteReach::Subgraph
    } else {
        RemoteReach::SingleEntity
    }
}

#[async_trait]
impl RemoteTemporalLinkedSource for LinkedForwarding {
    async fn remote_reach(&self, id: &Identifier, context: &ContextSpec) -> RemoteReach {
        match self.federation.temporal_retrieval_configurations(&self.live_target, id, context).await {
            Ok(configurations) => reach_of(&configurations),
            Err(error) => {
                warn!("Failed to read the registrations a linked temporal retrieval of {} reaches: {}", id, error);
                RemoteReach::SingleEntity
            }
        }
    }

    async fn fetch_linked_temporal(
        &self,
        id: &Identifier,
        remaining_depth: NonZeroU32,
        visited: &[Identifier],
        window: &TemporalLinkedWindow,
        context: &ContextSpec,
    ) -> Result<RemoteResolution<TemporalHistory>, TemporalError> {
        let parameters = linked_temporal_retrieval_parameters(remaining_depth, visited, window);
        let client_query = ClientQuery::TemporalRetrieve {
            id,
            params: &parameters,
            traversal: TemporalTraversal::Continued,
        };
        let request = linked_request(id, client_query, context);
        let forwarded = self.federation.forward_temporal_retrieve(&self.live_target, request).await;
        // Every source holding part of an entity's history contributes it; an
        // instance two of them answer is one instance.
        Ok(resolution(id, forwarded, |gathered: &mut Vec<TemporalHistory>, _, history| {
            match gathered.iter_mut().find(|held| held.id() == history.id()) {
                Some(held) => held.merge(history, MergeMode::Inclusive, window.timeproperty),
                None => gathered.push(history),
            }
        }))
    }

    async fn fetch_held_parts(
        &self,
        id: &Identifier,
        params: &TemporalQueryParameters,
        only: Option<&Identifier>,
        context: &ContextSpec,
    ) -> Result<RemoteResolution<HeldPart>, TemporalError> {
        let client_query = ClientQuery::TemporalRetrieve {
            id,
            params,
            traversal: TemporalTraversal::WalkedHere,
        };
        let request = ForwardRetrieveRequest {
            reach: only.map_or(SourceReach::Every, SourceReach::Only),
            ..linked_request(id, client_query, context)
        };
        let forwarded = self.federation.forward_temporal_retrieve(&self.live_target, request).await;
        Ok(resolution(id, forwarded, |held: &mut Vec<HeldPart>, registration, history| {
            held.push(HeldPart {
                registration: registration.cloned(), // clone: each part names the registration it came from
                history,
            });
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_forwarding::temporal::{linked_temporal_retrieval_parameters, reach_of};
    use andromeda_federation::source_identity::SourceConfiguration;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_linked_retrieval::source::RemoteReach;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel};
    use andromeda_ngsi_ld_query::temporal::aggregation_period::AggregationPeriod;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use andromeda_ngsi_ld_request::representation_parameter::WrittenValue;
    use andromeda_temporal::remote_linked::TemporalLinkedWindow;
    use std::{num::NonZeroU32, slice};

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    /// The plainest window: an open-ended `after` selection in the normalized
    /// representation.
    fn window() -> TemporalLinkedWindow {
        TemporalLinkedWindow {
            timerel: Some(TimeRel::After),
            time_at: Some("2024-01-01T00:00:00Z".parse().expect("valid instant")),
            end_time_at: None,
            timeproperty: TimeProp::Observed,
            last_n: None,
            format: TemporalFormat::Normalized,
            aggr_methods: None,
            aggr_period_duration: None,
            attrs: None,
        }
    }

    /// Every member of the window is restated on the forwarded retrieval, so
    /// the Linked Entity is answered over exactly the slice the requester
    /// asked for, and the traversal continuation rides beside it.
    #[test]
    fn a_full_window_is_restated_member_for_member_beside_the_continuation() {
        let window = TemporalLinkedWindow {
            timerel: Some(TimeRel::Between),
            time_at: Some("2024-01-01T00:00:00Z".parse().expect("valid instant")),
            end_time_at: Some("2024-02-01T00:00:00Z".parse().expect("valid instant")),
            timeproperty: TimeProp::Modified,
            last_n: NonZeroU32::new(5),
            format: TemporalFormat::AggregatedValues,
            aggr_methods: Some(vec![AggrMethod::Avg, AggrMethod::Max]),
            aggr_period_duration: AggregationPeriod::parse("PT1H").expect("valid duration"),
            attrs: None,
        };
        let visited = [uri("urn:ngsi-ld:Building:1")];

        let parameters = linked_temporal_retrieval_parameters(depth(3), &visited, &window);

        pretty_assertions::assert_eq!(parameters.timerel, Some(TimeRel::Between));
        pretty_assertions::assert_eq!(parameters.time_at, window.time_at);
        pretty_assertions::assert_eq!(parameters.end_time_at, window.end_time_at);
        pretty_assertions::assert_eq!(parameters.timeproperty, Some(TimeProp::Modified));
        pretty_assertions::assert_eq!(parameters.last_n.map(NonZeroU32::get), Some(5));
        pretty_assertions::assert_eq!(parameters.format, Some(WrittenValue::Supported(TemporalFormat::AggregatedValues)));
        pretty_assertions::assert_eq!(parameters.aggr_methods, Some(vec![AggrMethod::Avg, AggrMethod::Max]));
        pretty_assertions::assert_eq!(parameters.aggr_period_duration.map(|period| period.as_iso8601()), Some("PT1H".to_owned()));
        pretty_assertions::assert_eq!(parameters.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(parameters.join_level.map(NonZeroU32::get), Some(2));
        pretty_assertions::assert_eq!(parameters.contained_by.as_deref(), Some(visited.as_slice()));
    }

    /// The last level asks for the entity alone over the requester's window,
    /// and nothing the window does not hold -- an `attrs` or `datasetId` that
    /// narrowed the roots -- leaks into the traversal.
    #[test]
    fn the_last_level_forwards_the_window_alone() {
        let parameters = linked_temporal_retrieval_parameters(depth(1), &[], &window());

        pretty_assertions::assert_eq!(parameters.timerel, Some(TimeRel::After));
        assert!(parameters.join.is_none());
        assert!(parameters.join_level.is_none());
        assert!(parameters.contained_by.is_none());
        assert!(parameters.attrs.is_none());
        assert!(parameters.dataset_id.is_none());
    }

    /// A source advertising the extension resolves its own sub-graph; any
    /// other answer, or no answer, keeps the retrieval to the entity alone.
    #[test]
    fn remote_reach_is_subgraph_only_for_an_advertising_source() {
        let advertising = SourceConfiguration::Described(Some(
            JsonLiteral::admit(r#"{"extensions": ["temporalLinkedRetrieval"]}"#).expect("a JSON literal"),
        ));
        let other = SourceConfiguration::Described(Some(JsonLiteral::admit(r#"{"extensions": ["cursorPagination"]}"#).expect("a JSON literal")));
        pretty_assertions::assert_eq!(reach_of(slice::from_ref(&advertising)), RemoteReach::Subgraph);
        pretty_assertions::assert_eq!(reach_of(&[other]), RemoteReach::SingleEntity);
        pretty_assertions::assert_eq!(reach_of(&[SourceConfiguration::Described(None)]), RemoteReach::SingleEntity);
        pretty_assertions::assert_eq!(reach_of(&[SourceConfiguration::Undescribed]), RemoteReach::SingleEntity);
        pretty_assertions::assert_eq!(reach_of(&[]), RemoteReach::SingleEntity);
        pretty_assertions::assert_eq!(
            reach_of(&[advertising, SourceConfiguration::Undescribed]),
            RemoteReach::SingleEntity,
            "a retrieval reaching one non-advertising source asks for the entity alone"
        );
    }
}
