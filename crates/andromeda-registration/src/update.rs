//! Context-source registration update policy (ETSI GS CIM 009 clause 5.9.3).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    mode_rules::check_conflicts,
    payload::extract_update_fragment,
    resolved_update::apply_fragment,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistrationType,
};
use serde_json::Value;

impl RegistrationService {
    /// Merge an update fragment into an existing registration.
    pub async fn update(&self, id: &Uri, payload: Value, context: &ContextSpec) -> Result<(), RegistrationError> {
        // A fragment may omit `type`, but one it carries has to be the
        // registration type (clause 5.2.9).
        if let Some(type_value) = payload.get("type") {
            serde_json::from_value::<ContextSourceRegistrationType>(type_value.clone()) // clone: serde owns the member while the fragment remains borrowed.
                .map_err(|_| RegistrationError::csr_validation("type must be 'ContextSourceRegistration'"))?;
        }
        if payload.get("information").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("information must not be null"));
        }
        if payload.get("endpoint").is_some_and(Value::is_null) {
            return Err(RegistrationError::csr_validation("endpoint must not be null"));
        }

        let expanded_properties = expand_context_source_properties(&self.engine, &payload, context).await?;
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let existing = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let pre_update_snapshot =
            serde_json::to_value(&existing).map_err(|error| RegistrationError::internal(format!("Failed to serialize the pre-update CSR: {error}")))?;
        let mut fragment = extract_update_fragment(&payload)?;
        fragment.additional_properties = expanded_properties;
        let resolved = apply_fragment(existing, &fragment)?;
        // Clause 5.9.3.4 evaluates the rules against the registration as it
        // will be after the merge, not against the fragment alone.
        validate_shape(&RegistrationShape {
            mode: resolved.mode,
            information: &resolved.information,
            operations: &resolved.operations,
            context_source_info: resolved.context_source_info.as_deref(),
            expires_at: resolved.expires_at,
        })?;
        check_conflicts(self.repo.as_ref(), self.entity_probe.as_ref(), resolved.mode, &resolved.information, Some(id)).await?;
        self.repo.update(id, &resolved).await?.require_applied(id)?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the event owns the identifier borrowed from the request path.
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: Some(pre_update_snapshot),
        });
        Ok(())
    }
}
