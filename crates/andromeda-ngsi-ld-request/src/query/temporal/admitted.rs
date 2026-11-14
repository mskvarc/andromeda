//! A temporal query whose `q` filter request validation admitted.

use crate::query::{filter_not_admitted::FilterNotAdmitted, selection_mut::SelectionMut, temporal::TemporalQueryParameters};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{filter::compilation::AdmittedFilter, temporal::window::TimeInterval};
use derive_more::Deref;

/// The parameters of a temporal query together with the admission of their `q`
/// (clauses 5.5.6, 5.7.3.4, 5.7.4.4 and 5.14.5.4).
///
/// Validation makes one for Retrieve Temporal Evolution, Query Temporal
/// Evolution, the Entity Map it creates and the temporal delete-by-query, and
/// every compilation reads the filter from it, so a filter is admitted once,
/// by the request that wrote it. The admitted filter is always the admission of
/// the parameters' `q`: the parameters are read through
/// [`Deref`](std::ops::Deref) and changed only through operations that leave
/// the `q` and its admission together.
#[derive(Debug, Clone, Deref)]
pub struct AdmittedTemporalRequest {
    /// The parameters validation read.
    #[deref]
    params: TemporalQueryParameters,
    /// The admission of `params.q`, `None` exactly when `params.q` is.
    filter: Option<AdmittedFilter>,
}

impl AdmittedTemporalRequest {
    /// The parameters `filter` admitted the `q` of.
    pub(crate) const fn admitted(params: TemporalQueryParameters, filter: Option<AdmittedFilter>) -> AdmittedTemporalRequest {
        AdmittedTemporalRequest { params, filter }
    }

    /// The query of `params`, which name no `q` for an admission to cover.
    ///
    /// # Errors
    ///
    /// Returns [`FilterNotAdmitted`] when `params` carries a `q`.
    pub fn unfiltered(params: TemporalQueryParameters) -> Result<AdmittedTemporalRequest, FilterNotAdmitted> {
        match params.q {
            Some(_) => Err(FilterNotAdmitted),
            None => Ok(AdmittedTemporalRequest { params, filter: None }),
        }
    }

    /// The admission of the query's `q`, `None` when it carries none.
    #[must_use]
    pub const fn filter(&self) -> Option<&AdmittedFilter> {
        self.filter.as_ref()
    }

    /// The members an Entity Map holds the query to (clause 5.5.14).
    pub const fn selection_mut(&mut self) -> SelectionMut<'_> {
        SelectionMut {
            id: &mut self.params.id,
            entity_selectors: &mut self.params.entity_selectors,
        }
    }

    /// Ask the query of `entities` alone, entities its selection has already
    /// matched, keeping the filters and their admission.
    pub fn select_only(&mut self, entities: Vec<Identifier>) {
        self.params.select_only(entities);
    }

    /// Read the query over `interval` instead of the window it was asked with.
    pub const fn set_interval(&mut self, interval: TimeInterval) {
        self.params.set_interval(interval);
    }
}
