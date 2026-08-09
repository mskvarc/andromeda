//! Context-source registration deletion.

use crate::{error::AndromedaError, event::csr_change_event::CsrChangeEvent, service::csource_registration::CsourceRegistrationService};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::CsrTriggerReason;

impl CsourceRegistrationService {
    /// Delete a registration by URI.
    pub async fn delete(&self, id: &Uri) -> Result<(), AndromedaError> {
        let snapshot = self
            .repo
            .get(id)
            .await?
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| AndromedaError::internal(format!("Failed to serialize the deleted CSR: {error}")))?;
        if self.repo.delete(id).await? == 0 {
            return Err(AndromedaError::csr_not_found(id.as_str()));
        }
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the deletion event outlives the borrowed request identifier.
            trigger_reason: CsrTriggerReason::NoLongerMatching,
            csr_snapshot: snapshot,
        });
        Ok(())
    }
}
