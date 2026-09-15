//! What a temporal history delete-by-query must satisfy before it runs.

use crate::{
    error::ParameterError,
    query::temporal::delete_parameters::TemporalDeleteParameters,
    query_narrowing::{AdmittedNarrowings, NarrowingInputs, require_narrowing},
};
use andromeda_ngsi_ld_query::temporal::window::{BoundPresence, TimeInterval};

impl TemporalDeleteParameters {
    /// Validate the delete-by-query before it is compiled.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::SelectorRequired`] when the request carries no
    /// entity-narrowing input — a bare time window does not authorise deleting
    /// every entity's history, so the local scope is deliberately *not* counted
    /// as a narrowing here (the clause 5.6.21.4 rule with input (e) withheld).
    /// Returns [`ParameterError::DropKeepExclusive`] when both `drop` and `keep`
    /// are present, [`ParameterError::DatasetIdWithDeleteAll`] when `datasetId`
    /// and `deleteAll` are both present, and [`ParameterError::Query`] carrying
    /// the clause 5.7.3 window fault when `timerel`/`timeAt` are half-specified
    /// or `between` came without `endTimeAt`.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // A delete must name what to select entities by. The local scope is
        // withheld from the admitted narrowings: the delete is always local, so
        // counting the local scope would make every windowed request narrow and
        // let a bare window delete everyone's history.
        require_narrowing(
            &NarrowingInputs::builder()
                .admitted(AdmittedNarrowings::ENTITIES)
                .entity_type(self.entity_type.as_ref())
                .purged(self.scoped_attributes())
                .q(self.q.as_ref())
                .geo(self.geo.0.as_ref())
                .build(),
        )?;

        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }

        // Clause 5.6.13: `deleteAll` removes every instance and `datasetId`
        // names one, so the pair is contradictory.
        if self.deletes_all_datasets() && self.dataset_id.is_some() {
            return Err(ParameterError::DatasetIdWithDeleteAll);
        }

        // Clause 5.7.3: `timerel`/`timeAt` are meaningless apart, and `between`
        // requires `endTimeAt`. Checked on presence before the instants are
        // read, exactly as the temporal query's own window check is.
        TimeInterval::check_time_bounds(
            self.timerel,
            BoundPresence::of(self.time_at.as_ref()),
            BoundPresence::of(self.end_time_at.as_ref()),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ParameterError, params_from_query, query::temporal::delete_parameters::TemporalDeleteParameters};
    use andromeda_ngsi_ld_query::error::QueryError;

    fn params(query: &str) -> TemporalDeleteParameters {
        params_from_query!(query, TemporalDeleteParameters)
    }

    /// Clause 5.6.21.4 with input (e) withheld: a window alone does not narrow a
    /// delete, so a request carrying only a window is too wide.
    #[test]
    fn a_window_only_delete_is_too_wide() {
        let err = params("timerel=before&timeAt=2030-01-01T00:00:00Z")
            .validate()
            .expect_err("a bare window authorises deleting everyone");
        assert!(matches!(err, ParameterError::SelectorRequired(_)), "got {err:?}");
    }

    /// Each entity-narrowing input authorises a delete on its own.
    #[test]
    fn a_type_selector_or_drop_or_query_narrows_the_delete() {
        for query in ["type=Vehicle", "drop=speed", "q=speed%3E10"] {
            assert!(params(query).validate().is_ok(), "{query} must narrow the delete");
        }
    }

    #[test]
    fn drop_and_keep_together_are_rejected() {
        let err = params("type=Vehicle&drop=speed&keep=fuel").validate().expect_err("drop and keep contradict");
        assert!(matches!(err, ParameterError::DropKeepExclusive), "got {err:?}");
    }

    #[test]
    fn dataset_id_with_delete_all_is_rejected() {
        let err = params("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:1&deleteAll=true")
            .validate()
            .expect_err("datasetId and deleteAll contradict");
        assert!(matches!(err, ParameterError::DatasetIdWithDeleteAll), "got {err:?}");
    }

    /// Clause 5.7.3: `timerel=between` needs `endTimeAt`.
    #[test]
    fn a_between_window_needs_an_end_time() {
        let err = params("type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z")
            .validate()
            .expect_err("between needs endTimeAt");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamEndTimeAtRequired)), "got {err:?}");
        assert!(
            params("type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-06-01T00:00:00Z")
                .validate()
                .is_ok()
        );
    }

    /// Clause 5.7.3: `timerel` and `timeAt` are meaningless apart.
    #[test]
    fn a_half_specified_window_is_rejected() {
        let err = params("type=Vehicle&timerel=after").validate().expect_err("timerel needs timeAt");
        assert!(matches!(err, ParameterError::Query(QueryError::ParamTimerelTimeatPair)), "got {err:?}");
    }

    /// A read-only temporal parameter the delete cannot use is refused while the
    /// query string is decoded (clause 6.3.20 pattern).
    #[test]
    fn a_read_only_parameter_is_refused_at_decode() {
        assert!(serde_urlencoded::from_str::<TemporalDeleteParameters>("type=Vehicle&lastN=5").is_err());
        assert!(serde_urlencoded::from_str::<TemporalDeleteParameters>("type=Vehicle&format=temporalValues").is_err());
    }
}
