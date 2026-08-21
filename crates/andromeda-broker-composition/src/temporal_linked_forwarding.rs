//! Resolving a Linked Entity's temporal evolution through the registered
//! context sources.
//!
//! Clause 4.5.23.1 lets a Relationship annotate its object with an
//! `objectType`, which declares that object an Internal Linked Entity: an
//! entity of the declared type that exists in the distributed system, whether
//! or not this broker records its history. The traversal in `andromeda-temporal`
//! answers for the history it holds and asks this source for the rest; this is
//! where "the rest" becomes a forwarded temporal retrieval against the matching
//! Context Source Registrations.
//!
//! The forwarded retrieval carries the requesting client's own window, so the
//! roots and their Linked Entities describe one consistent slice of history
//! rather than being spliced together from two different ones.

use crate::{linked_resolution_failure_mapping::resolution_failure, linked_traversal_parameters::append_traversal};
use andromeda_federation::{
    FederationService,
    operation::ForwardingOperation,
    request::{CsrModeFilter, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use andromeda_temporal::{
    error::TemporalError,
    remote_linked::{RemoteTemporalLinkedEntities, RemoteTemporalLinkedSource, TemporalLinkedWindow},
};
use async_trait::async_trait;
use chrono::SecondsFormat;
use std::{num::NonZeroU32, sync::Arc};
use tracing::warn;
use url::form_urlencoded;

/// Fetches the temporal evolution of an `objectType`-annotated Linked Entity
/// from the context sources registered for its identifier (clause 4.5.23.1).
#[derive(derive_more::Debug)]
pub(crate) struct ForwardedTemporalLinkedSource {
    /// The forwarding service that owns registration matching and transport.
    #[debug(skip)]
    federation: Arc<FederationService>,
}

impl ForwardedTemporalLinkedSource {
    /// Resolve Linked Entities' temporal evolution through the given forwarding
    /// service.
    pub(crate) const fn new(federation: Arc<FederationService>) -> ForwardedTemporalLinkedSource {
        ForwardedTemporalLinkedSource { federation }
    }
}

/// Append the window the forwarded retrieval must reproduce.
///
/// Every member the requester used to select its own slice of history is
/// restated, because a Linked Entity answered over a different window would
/// splice two unrelated slices into one response. `format=normalized` is the
/// absence of a format rather than a value clause 6.3.12 accepts, so it is
/// never written.
fn append_window(serializer: &mut form_urlencoded::Serializer<'_, String>, window: &TemporalLinkedWindow) {
    if let Some(timerel) = window.timerel {
        serializer.append_pair("timerel", &timerel.to_string());
    }
    if let Some(time_at) = window.time_at {
        serializer.append_pair("timeAt", &time_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    if let Some(end_time_at) = window.end_time_at {
        serializer.append_pair("endTimeAt", &end_time_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    serializer.append_pair("timeproperty", &window.timeproperty.to_string());
    if let Some(last_n) = window.last_n {
        serializer.append_pair("lastN", &last_n.to_string());
    }
    match window.format {
        TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues => {
            serializer.append_pair("format", &window.format.to_string());
        }
        TemporalFormat::Normalized => {}
    }
    if let Some(methods) = window.aggr_methods.as_deref()
        && !methods.is_empty()
    {
        let joined = methods.iter().map(AggrMethod::to_string).collect::<Vec<_>>().join(",");
        serializer.append_pair("aggrMethods", &joined);
    }
    if let Some(period) = window.aggr_period_duration {
        serializer.append_pair("aggrPeriodDuration", &period.as_iso8601());
    }
}

/// Build the query string of the forwarded temporal retrieval: the window the
/// answer must reproduce, followed by the traversal continuation.
fn linked_temporal_retrieval_query(depth: NonZeroU32, visited: &[Uri], window: &TemporalLinkedWindow) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    append_window(&mut serializer, window);
    append_traversal(&mut serializer, depth, visited);
    serializer.finish()
}

#[async_trait]
impl RemoteTemporalLinkedSource for ForwardedTemporalLinkedSource {
    async fn fetch_linked_temporal(
        &self,
        id: &Uri,
        remaining_depth: NonZeroU32,
        visited: &[Uri],
        window: &TemporalLinkedWindow,
        context: &ContextSpec,
    ) -> Result<RemoteTemporalLinkedEntities, TemporalError> {
        let query_string = linked_temporal_retrieval_query(remaining_depth, visited, window);
        let forwarded = self
            .federation
            .forward_temporal_retrieve(ForwardRetrieveRequest {
                entity_id: id,
                exclude_registration: None,
                query_string: &query_string,
                context,
                operation: ForwardingOperation::RetrieveTemporalEntity,
                // A linked retrieval starts at this broker rather than being a
                // hop of an inbound one, so there is no `Via` chain to extend
                // and no headers to pass through.
                triggering: TriggeringRequest::default(),
                // Clause 4.3.6.2: an auxiliary context source only supplements
                // an entity that already exists, never supplies a missing one.
                mode_filter: CsrModeFilter::ExcludeAuxiliary,
                entity_map: None,
            })
            .await;

        // A Linked Entity that cannot be reached degrades the join instead of
        // the request (clause 4.5.23): the Relationship keeps its plain URI and
        // the caller is told what is missing.
        match forwarded {
            Ok(sources) => {
                let mut resolved = RemoteTemporalLinkedEntities::default();
                for source in sources {
                    for warning in source.warnings {
                        warn!("Linked entity {} could not be fully resolved from a context source: {}", id, warning);
                        resolved.failures.push(resolution_failure(id, warning));
                    }
                    resolved.entities.extend(source.entities);
                }
                Ok(resolved)
            }
            Err(error) => {
                warn!("Failed to forward the linked temporal retrieval of {}: {}", id, error);
                Ok(RemoteTemporalLinkedEntities {
                    entities: Vec::new(),
                    failures: vec![LinkedResolutionFailure {
                        id: id.clone(), // clone: the failure outlives the borrowed candidate id
                        kind: LinkedResolutionFailureKind::Unreachable,
                        detail: error.to_string(),
                    }],
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal_linked_forwarding::linked_temporal_retrieval_query;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel, uri::Uri};
    use andromeda_ngsi_ld_query::temporal::aggregation_period::AggregationPeriod;
    use andromeda_temporal::remote_linked::TemporalLinkedWindow;
    use std::num::NonZeroU32;
    use url::form_urlencoded;

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    fn decode(query: &str) -> Vec<(String, String)> {
        form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
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
        }
    }

    /// Three levels left means the fetched entity plus two more hops, so the
    /// remote broker is asked for `joinLevel=2`, and the already-walked ids
    /// travel as `containedBy` (the clause 4.5.23 containedBy query parameter) — behind the window the answer
    /// must be taken over.
    #[test]
    fn the_forwarded_retrieval_joins_one_level_less_than_the_depth_that_remains() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let query = linked_temporal_retrieval_query(depth(3), &visited, &window());

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![
                ("timerel".to_owned(), "after".to_owned()),
                ("timeAt".to_owned(), "2024-01-01T00:00:00Z".to_owned()),
                ("timeproperty".to_owned(), "observedAt".to_owned()),
                ("join".to_owned(), "flat".to_owned()),
                ("joinLevel".to_owned(), "2".to_owned()),
                ("containedBy".to_owned(), "urn:ngsi-ld:Building:1,urn:ngsi-ld:Building:2".to_owned()),
            ]
        );
    }

    /// The last level asks for the entity alone: the clause 4.5.23 joinLevel
    /// query parameter has no zero-hop
    /// join, so the forwarded retrieval carries no `join` parameter — but still
    /// carries the window, without which the answer would be a different slice
    /// of history.
    #[test]
    fn the_last_level_forwards_a_plain_temporal_retrieval() {
        let query = linked_temporal_retrieval_query(depth(1), &[uri("urn:ngsi-ld:Building:1")], &window());

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![
                ("timerel".to_owned(), "after".to_owned()),
                ("timeAt".to_owned(), "2024-01-01T00:00:00Z".to_owned()),
                ("timeproperty".to_owned(), "observedAt".to_owned()),
                ("containedBy".to_owned(), "urn:ngsi-ld:Building:1".to_owned()),
            ]
        );
    }

    /// A traversal that has visited nothing yet sends no `containedBy` at all,
    /// rather than an empty one.
    #[test]
    fn an_empty_visited_list_is_omitted() {
        let query = linked_temporal_retrieval_query(depth(2), &[], &window());

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![
                ("timerel".to_owned(), "after".to_owned()),
                ("timeAt".to_owned(), "2024-01-01T00:00:00Z".to_owned()),
                ("timeproperty".to_owned(), "observedAt".to_owned()),
                ("join".to_owned(), "flat".to_owned()),
                ("joinLevel".to_owned(), "1".to_owned()),
            ]
        );
    }

    /// Every member of a closed, aggregated, truncated window is restated, so
    /// the Linked Entity is answered over exactly the slice the requester asked
    /// for.
    #[test]
    fn a_full_window_is_restated_member_for_member() {
        let window = TemporalLinkedWindow {
            timerel: Some(TimeRel::Between),
            time_at: Some("2024-01-01T00:00:00Z".parse().expect("valid instant")),
            end_time_at: Some("2024-02-01T00:00:00Z".parse().expect("valid instant")),
            timeproperty: TimeProp::Modified,
            last_n: Some(5),
            format: TemporalFormat::AggregatedValues,
            aggr_methods: Some(vec![AggrMethod::Avg, AggrMethod::Max]),
            aggr_period_duration: AggregationPeriod::parse("PT1H").expect("valid duration"),
        };

        let query = linked_temporal_retrieval_query(depth(1), &[], &window);

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![
                ("timerel".to_owned(), "between".to_owned()),
                ("timeAt".to_owned(), "2024-01-01T00:00:00Z".to_owned()),
                ("endTimeAt".to_owned(), "2024-02-01T00:00:00Z".to_owned()),
                ("timeproperty".to_owned(), "modifiedAt".to_owned()),
                ("lastN".to_owned(), "5".to_owned()),
                ("format".to_owned(), "aggregatedValues".to_owned()),
                ("aggrMethods".to_owned(), "avg,max".to_owned()),
                ("aggrPeriodDuration".to_owned(), "PT1H".to_owned()),
            ]
        );
    }

    /// `normalized` is the absence of a `format`, not a value clause 6.3.12
    /// accepts, so it is never written onto the forwarded retrieval.
    #[test]
    fn the_normalized_representation_writes_no_format_parameter() {
        let query = linked_temporal_retrieval_query(depth(1), &[], &window());

        assert!(!query.contains("format"), "unexpected format parameter: {query}");
    }

    /// Every reserved character the identifiers carry is percent-encoded by the
    /// serializer, so the comma-joined `containedBy` list survives the round
    /// trip as one parameter rather than several.
    #[test]
    fn the_visited_identifiers_are_percent_encoded_into_one_parameter() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let query = linked_temporal_retrieval_query(depth(2), &visited, &window());

        assert!(
            query.contains("containedBy=urn%3Angsi-ld%3ABuilding%3A1%2Curn%3Angsi-ld%3ABuilding%3A2"),
            "unexpected encoding: {query}"
        );
    }
}
