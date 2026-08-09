//! Context-source registration deletion policy.

use crate::{RegistrationService, error::RegistrationError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};

impl RegistrationService {
    /// Delete a registration by URI.
    pub async fn delete(&self, id: &Uri) -> Result<(), RegistrationError> {
        let snapshot = self
            .repo
            .get(id)
            .await?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| RegistrationError::internal(format!("Failed to serialize the deleted CSR: {error}")))?;
        self.repo.delete(id).await?.require_applied(id)?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the deletion event outlives the borrowed request identifier.
            trigger_reason: CsrTriggerReason::NoLongerMatching,
            csr_snapshot: snapshot,
        });
        Ok(())
    }
}
