//! The page a temporal query answers with, cut from the merged history.
//!
//! Clause 5.5.9 cuts the requested window from the merged collection once
//! context sources contribute, rather than from the local rows.

use crate::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};

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
fn merged_page(
    entities: Vec<TemporalHistory>,
    params: &TemporalQueryParameters,
    window: &PageWindow,
    count: CountPolicy,
) -> (Vec<TemporalHistory>, Option<usize>) {
    let mut bounded = entities;
    if let Some(last_n) = params.last_n {
        let timeproperty = params.timeproperty_or_default();
        for entity in &mut bounded {
            entity.keep_last(timeproperty, last_n);
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

/// The identifier a merged temporal entity is ordered by; one that names none
/// has the empty key and sorts first.
fn identifier_key(entity: &TemporalHistory) -> &str {
    entity.id().map_or("", Identifier::as_str)
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
pub(crate) fn cut_merged_page(
    merged_entities: Vec<TemporalHistory>,
    cut: MergedPageCut<'_>,
    params: &TemporalQueryParameters,
) -> (Vec<TemporalHistory>, Option<usize>) {
    let MergedPageCut { window, count, local_total } = cut;
    match window {
        Some(window) => merged_page(merged_entities, params, window, count),
        None => (merged_entities, local_total),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal::{
            TemporalQueryParameters,
            query::merged_page::{MergedPageCut, cut_merged_page},
        },
        test_history::held_history,
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};
    use serde_json::json;

    /// An entity whose `speed` was observed once a day for five days.
    async fn observed_daily(id: &str) -> TemporalHistory {
        let speed: Vec<_> = (1..=5)
            .map(|day| json!({"type": "Property", "value": day, "observedAt": format!("2024-01-0{day}T00:00:00Z")}))
            .collect();
        held_history(json!({"id": id, "type": "Vehicle", "https://example.com/speed": speed})).await
    }

    /// Clause 5.7.4.4 (T-D4): `lastN` bounds the instances of the merged
    /// history, keeping the most recent ones, and every response lists its
    /// instances oldest first.
    #[tokio::test]
    async fn last_n_after_merge_keeps_ascending_order() {
        let params = TemporalQueryParameters {
            last_n: Some(3),
            ..TemporalQueryParameters::default()
        };
        let window = PageWindow::unbounded();
        let (page, total) = cut_merged_page(
            vec![observed_daily("urn:ngsi-ld:Vehicle:2").await, observed_daily("urn:ngsi-ld:Vehicle:1").await],
            MergedPageCut {
                window: Some(&window),
                count: CountPolicy::Emit,
                local_total: None,
            },
            &params,
        );
        assert_eq!(total, Some(2));
        assert_eq!(
            page.iter().filter_map(|entity| entity.id().map(ToString::to_string)).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
        );
        for entity in &page {
            let TemporalHistory::Instances(document) = entity else {
                panic!("a history of instances");
            };
            let days: Vec<_> = document
                .instances()
                .filter_map(|(_, instance)| instance.instant_on(TimeProp::Observed))
                .collect();
            assert_eq!(days.len(), 3);
            assert!(days.windows(2).all(|pair| pair[0] < pair[1]), "{days:?}");
            assert_eq!(days[2].to_rfc3339(), "2024-01-05T00:00:00+00:00");
        }
    }
}
