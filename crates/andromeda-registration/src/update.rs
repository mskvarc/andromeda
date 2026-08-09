//! Context-source registration update policy.

use crate::{
    RegistrationService,
    error::RegistrationError,
    payload::extract_update_fragment,
    resolved_update::apply_fragment,
    validation::validate_common_members,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};
use serde_json::Value;

impl RegistrationService {
    /// Merge an update fragment into an existing registration.
    pub async fn update(&self, id: &Uri, payload: Value, context: &ContextSpec) -> Result<(), RegistrationError> {
        validate_common_members(&payload)?;
        if payload.get("information").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("information must not be null"));
        }
        if payload.get("endpoint").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("endpoint must not be null"));
        }

        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let existing = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let pre_update_snapshot =
            serde_json::to_value(&existing).map_err(|error| RegistrationError::internal(format!("Failed to serialize the pre-update CSR: {error}")))?;
        let fragment = extract_update_fragment(&payload)?;
        let resolved = apply_fragment(existing, &fragment)?;
        self.repo.update(id, &resolved).await?.require_applied(id)?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the event owns the identifier borrowed from the request path.
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: Some(pre_update_snapshot),
        });
        Ok(())
    }
}
