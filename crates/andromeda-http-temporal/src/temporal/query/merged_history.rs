//! Folding what the context sources returned into the locally read history
//! (clause 5.7.4.4).

use crate::temporal::{
    ForwardingMode,
    ProblemResponse,
    collect_temporal_warnings,
    query::forwarding::{ForwardedHistory, too_many_results},
};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    config::SplitEntities,
    entity_result::temporal_contributions_of,
    outcome::ForwardedTemporalEntities,
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use axum::http::Uri;
use serde_json::Value;

/// The merged history of a temporal query, and what the sources contributed to it.
pub(crate) struct MergedTemporalHistory {
    /// The local rows folded together with what every contacted source returned.
    pub(crate) entities: Vec<Value>,
    /// The clause 6.3.17 warnings the forwarding produced.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// Which registration returned which entity, and under which map, captured
    /// before the merge erased the boundary (clauses 5.2.39 and 5.5.14).
    pub(crate) contributions: Vec<SourceContribution>,
}

/// Fold what the context sources returned into the local rows.
///
/// Clause 5.7.4.4: under `splitEntities` an auxiliary source holds part of an
/// entity the broker also holds, so its contribution joins the aggregation
/// instead of only filling gaps.
///
/// # Errors
///
/// The clause 4.12 `TooManyResults` refusal when the sources contributed and
/// the merged collection exceeds the budget they were walked under: the local
/// rows were read one past it, so a larger collection is refused rather than
/// paged from a truncation.
pub(crate) fn merge_forwarded_history(
    local_entities: Vec<Value>,
    forwarded: Option<ForwardedHistory>,
    filtering: SplitEntities,
    instance: &Uri,
) -> Result<MergedTemporalHistory, ProblemResponse> {
    let Some(history) = forwarded else {
        return Ok(MergedTemporalHistory {
            entities: local_entities,
            warnings: Vec::new(),
            contributions: Vec::new(),
        });
    };
    let contributed = history.holds_remote_entities();
    let ForwardedHistory { sources, budget } = history;
    let warnings = collect_temporal_warnings(&sources);
    let contributions: Vec<SourceContribution> = temporal_contributions_of(&sources).collect();
    let sources: Vec<ForwardedTemporalEntities> = match filtering {
        SplitEntities::Split => sources
            .into_iter()
            .map(|mut result| {
                if result.mode == ForwardingMode::Auxiliary {
                    result.mode = ForwardingMode::Inclusive;
                }
                result
            })
            .collect(),
        SplitEntities::Complete => sources,
    };
    let entities = merge_temporal_query_results(local_entities, sources);
    if contributed {
        budget
            .admits(u64::try_from(entities.len()).unwrap_or(u64::MAX))
            .map_err(|exceeded| too_many_results(exceeded, instance))?;
    }
    Ok(MergedTemporalHistory {
        entities,
        warnings,
        contributions,
    })
}
