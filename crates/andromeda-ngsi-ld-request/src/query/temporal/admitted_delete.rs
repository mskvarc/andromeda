//! A temporal history delete-by-query whose selection `q` request validation
//! admitted.

use crate::{
    error::ParameterError,
    query::temporal::{admitted::AdmittedTemporalRequest, delete_parameters::TemporalDeleteParameters},
};
use andromeda_linked_retrieval::{admission::LinkedOperation, filter_hops::FilterHops};
use andromeda_ngsi_ld_query::filter::compilation::{FilterAdmission, admit_q_filter};
use derive_more::Deref;

/// A temporal delete-by-query together with the temporal query its entity
/// selection and window describe, the `q` of which was admitted.
///
/// The delete takes no `join`, so it refuses every hop of its `q` as Purge
/// Entities does (clause 5.6.21.4).
#[derive(Debug, Deref)]
pub struct AdmittedTemporalDelete {
    /// What the delete removes and how it is scoped.
    #[deref]
    params: TemporalDeleteParameters,
    /// The query that resolves the entities the delete addresses.
    selection: AdmittedTemporalRequest,
}

impl AdmittedTemporalDelete {
    /// The temporal query resolving the entities the delete addresses.
    #[must_use]
    pub const fn selection(&self) -> &AdmittedTemporalRequest {
        &self.selection
    }
}

impl TemporalDeleteParameters {
    /// Validate the delete-by-query and admit the `q` of its selection under
    /// `admission`, the broker's complexity limits and `q_target_languages`
    /// level.
    ///
    /// # Errors
    ///
    /// Returns every refusal of [`TemporalDeleteParameters::validate`],
    /// [`ParameterError::LinkedRetrievalUnavailable`] when the `q` traverses a
    /// Linked Entity, and [`ParameterError::Query`] when it is too complex
    /// (clause 5.5.6) or names a target language `admission` does not admit.
    pub fn admit(self, admission: FilterAdmission) -> Result<AdmittedTemporalDelete, ParameterError> {
        self.validate()?;
        let selection = self.selection_params();
        let hops = FilterHops::new(LinkedOperation::TemporalDeleteByQuery, None, None);
        let filter = selection
            .q
            .as_ref()
            .map(|q| admit_q_filter::<ParameterError, _>(q, admission, &hops))
            .transpose()?;
        Ok(AdmittedTemporalDelete {
            params: self,
            selection: AdmittedTemporalRequest::admitted(selection, filter),
        })
    }
}
