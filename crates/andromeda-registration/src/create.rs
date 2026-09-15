//! Context-source registration creation policy (ETSI GS CIM 009 clause 5.9.2).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    identifier::generate_registration_id,
    mode_rules::check_conflicts,
    payload::extract_create_params,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::{context_spec::ContextSpec, scoped_context_admission::reject_scoped_user_contexts};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};
use serde_json::Value;

impl RegistrationService {
    /// Create a context-source registration from a JSON-LD payload.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Validation`] when `id` is not a URI or a
    /// member of the payload falls outside Table 5.2.9-1,
    /// [`RegistrationError::AlreadyExists`] when the payload names an
    /// identifier the store already holds, [`RegistrationError::Conflict`]
    /// when a proxied footprint overlaps an existing registration or context
    /// data the broker already holds (clause 5.9.2.4),
    /// [`RegistrationError::JsonLd`] when the request context cannot be loaded
    /// or the payload cannot be expanded, [`RegistrationError::Identifier`]
    /// when no identifier could be generated for an anonymous registration,
    /// and [`RegistrationError::Persistence`] when the store rejects the
    /// write.
    pub async fn create(&self, payload: Value, context: &ContextSpec) -> Result<Uri, RegistrationError> {
        let id = match payload.get("id").and_then(Value::as_str) {
            Some(raw) => {
                let id = raw.parse::<Uri>().map_err(|_| RegistrationError::csr_validation("id must be a valid URI"))?;
                if self.repo.exists(&id).await? {
                    return Err(RegistrationError::already_exists(&id));
                }
                id
            }
            None => generate_registration_id()?,
        };

        // Clause 5.5.7: the request `@context` a registration is created under
        // shall not declare a JSON-LD Scoped Context, refused before the payload
        // is expanded against it.
        reject_scoped_user_contexts(&self.engine, context).await?;

        let validation_document = serde_json::json!({
            "id": "urn:ngsi-ld:_:context-validate",
            "type": "ContextSourceRegistration"
        });
        self.engine.expand(validation_document, context).await?;

        let expanded_properties = expand_context_source_properties(&self.engine, &payload, context).await?;
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let mut params = extract_create_params(&payload)?;
        params.additional_properties = expanded_properties;
        validate_shape(&RegistrationShape {
            mode: params.mode,
            information: &params.information,
            operations: &params.operations,
            context_source_info: params.context_source_info.as_deref(),
            expires_at: params.expires_at,
        })?;
        check_conflicts(self.repo.as_ref(), self.entity_probe.as_ref(), params.mode, &params.information, None).await?;
        self.repo.create(&id, &params).await?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the change event and response each own the identifier.
            trigger_reason: CsrTriggerReason::NewlyMatching,
            csr_snapshot: None,
        });
        Ok(id)
    }
}
