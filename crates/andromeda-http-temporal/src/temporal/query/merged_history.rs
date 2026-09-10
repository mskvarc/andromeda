//! Folding what the context sources returned into the locally read history
//! (clause 5.7.4.4).

use crate::temporal::{ForwardingMode, collect_temporal_warnings};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    config::SplitEntities,
    entity_result::temporal_contributions_of,
    outcome::ForwardedTemporalEntities,
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
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
pub(crate) fn merge_forwarded_history(
    local_entities: Vec<Value>,
    forwarded: Option<Vec<ForwardedTemporalEntities>>,
    filtering: SplitEntities,
) -> MergedTemporalHistory {
    let Some(forwarded) = forwarded else {
        return MergedTemporalHistory {
            entities: local_entities,
            warnings: Vec::new(),
            contributions: Vec::new(),
        };
    };
    let warnings = collect_temporal_warnings(&forwarded);
    let contributions: Vec<SourceContribution> = temporal_contributions_of(&forwarded).collect();
    let forwarded: Vec<ForwardedTemporalEntities> = match filtering {
        SplitEntities::Split => forwarded
            .into_iter()
            .map(|mut result| {
                if result.mode == ForwardingMode::Auxiliary {
                    result.mode = ForwardingMode::Inclusive;
                }
                result
            })
            .collect(),
        SplitEntities::Complete => forwarded,
    };
    MergedTemporalHistory {
        entities: merge_temporal_query_results(local_entities, forwarded),
        warnings,
        contributions,
    }
}
