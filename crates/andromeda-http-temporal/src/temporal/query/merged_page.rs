//! The page a temporal query answers with, cut from the merged history.
//!
//! Clause 5.5.9 cuts the requested window from the merged collection once
//! context sources contribute, rather than from the local rows.

use crate::temporal::TemporalQueryParameters;
use andromeda_federation::temporal_last_n::truncate_to_last_n;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};
use serde_json::Value;

/// Bound the merged history and cut the requested page from it.
///
/// Clause 5.7.4.4: `lastN` bounds the instances of the aggregated history, so
/// it is applied once more over the merged entities; each source only bounded
/// its own part. Clause 5.5.9 then takes the page from the merged collection
/// rather than from the local rows, and clause 6.3.13 counts the merged
/// matches rather than the page.
///
/// The merge holds the local rows and then what each source answered, in
/// arrival order, and paging needs one deterministic order: the default `id`
/// order the local store pages in, byte-wise as the store collates
/// identifiers. No other order reaches this point, because clause 5.7.4.4
/// refuses an ordering on a query not limited to the local scope. The window
/// is the one the client asked for, by offset or by cursor alike.
fn merged_page(entities: Vec<Value>, params: &TemporalQueryParameters, window: &PageWindow, count: CountPolicy) -> (Vec<Value>, Option<usize>) {
    let mut bounded = entities;
    if let Some(last_n) = params.last_n {
        let timeproperty = params.timeproperty_or_default();
        for entity in &mut bounded {
            truncate_to_last_n(entity, last_n, timeproperty);
        }
    }
    bounded.sort_by(|left, right| identifier_key(left).cmp(identifier_key(right)));
    let bounded = bounded;
    let total = match count {
        CountPolicy::Emit => Some(bounded.len()),
        CountPolicy::Omit => None,
    };
    (window.cut(bounded, identifier_key), total)
}

/// The identifier a merged temporal entity is ordered by; one without a
/// readable `id` has the empty key and sorts first.
fn identifier_key(entity: &Value) -> &str {
    entity.get("id").and_then(Value::as_str).unwrap_or_default()
}

/// What the page cut of a temporal query reads once the context sources have
/// answered.
#[derive(Clone, Copy)]
pub(crate) struct MergedPageCut<'page> {
    /// The page window withheld while the merge was pending, absent when no
    /// source contributed and the local query already cut the page.
    pub(crate) window: Option<&'page PageWindow>,
    /// Whether the client asked for the total of the merged collection
    /// (clause 6.3.13).
    pub(crate) count: CountPolicy,
    /// The match count the local read reported, which stands when no page was
    /// withheld.
    pub(crate) local_total: Option<usize>,
}

/// Cut the page the client asked for from the merged collection.
pub(crate) fn cut_merged_page(merged_entities: Vec<Value>, cut: MergedPageCut<'_>, params: &TemporalQueryParameters) -> (Vec<Value>, Option<usize>) {
    let MergedPageCut { window, count, local_total } = cut;
    match window {
        Some(window) => merged_page(merged_entities, params, window, count),
        None => (merged_entities, local_total),
    }
}
