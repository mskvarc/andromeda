//! Reusing the temporal query machinery to resolve a delete-by-query's entities.
//!
//! Entity selection and the time window are identical to a temporal query's, so
//! rather than duplicate the compilation the delete lifts its selection into a
//! [`TemporalQueryParameters`] and reuses `compile_temporal_query`. The
//! attribute `drop`/`keep`, the `datasetId`/`deleteAll` scope and the `dryRun`
//! flag are the delete's own and stay on [`TemporalDeleteParameters`]; only what
//! the shared compiler reads is carried across.

use crate::query::temporal::{TemporalQueryParameters, delete_parameters::TemporalDeleteParameters};

impl TemporalDeleteParameters {
    /// The temporal query the delete's entity selection and window describe.
    ///
    /// Every member the delete does not carry is left at its default, and the
    /// execution is pinned to the local scope: the delete is never forwarded, so
    /// its entity set is resolved against this broker's history alone.
    #[must_use]
    pub fn selection_params(&self) -> TemporalQueryParameters {
        TemporalQueryParameters {
            // clone: the compiled selection owns its filters independently of
            // the reusable delete parameters, which the scope still reads.
            id: self.id.clone(),
            entity_type: self.entity_type.clone(),
            id_pattern: self.id_pattern.clone(),
            q: self.q.clone(),
            geo: self.geo.clone(),
            scope_q: self.scope_q.clone(),
            csf: self.csf.clone(),
            timerel: self.timerel,
            time_at: self.time_at,
            end_time_at: self.end_time_at,
            timeproperty: self.timeproperty,
            dataset_id: self.dataset_id.clone(),
            // The delete is local-only; it is never forwarded to a Context Source.
            local: Some(true),
            ..TemporalQueryParameters::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{params_from_query, query::temporal::delete_parameters::TemporalDeleteParameters};
    use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};

    fn params(query: &str) -> TemporalDeleteParameters {
        params_from_query!(query, TemporalDeleteParameters)
    }

    /// The selection and window cross into the reused query; the local scope is
    /// pinned so entity resolution never forwards.
    #[test]
    fn the_selection_and_window_are_carried_into_the_reused_query() {
        let selection = params("type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z&timeproperty=modifiedAt").selection_params();
        assert!(selection.entity_type.is_some());
        pretty_assertions::assert_eq!(selection.timerel, Some(TimeRel::After));
        pretty_assertions::assert_eq!(selection.timeproperty, Some(TimeProp::Modified));
        pretty_assertions::assert_eq!(selection.local, Some(true));
    }

    /// The read-only shaping a delete cannot use is absent from the reused
    /// query, because the delete parameters never carried it.
    #[test]
    fn the_reused_query_carries_no_read_only_shaping() {
        let selection = params("type=Vehicle&drop=speed").selection_params();
        assert!(selection.last_n.is_none());
        assert!(selection.format.is_none());
        assert!(selection.pick.is_none());
        assert!(selection.join.is_none());
        // `drop` is the delete's own scope, not a positive attribute filter.
        assert!(selection.attrs.is_none());
    }
}
