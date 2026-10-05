//! Context-source registration creation policy (ETSI GS CIM 009 clause 5.9.2).

use crate::{
    RegistrationService,
    context_source_properties::expand_context_source_properties,
    error::RegistrationError,
    geometry::admit_registration_geometries,
    identifier::{client_registration_id, generate_registration_id},
    mode_rules::check_conflicts,
    payload::extract_create_params,
    validation::{RegistrationShape, validate_shape},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
    /// [`RegistrationError::JsonLd`] when the request context cannot be
    /// processed or the payload cannot be expanded, [`RegistrationError::Internal`]
    /// when no identifier could be minted for an anonymous registration,
    /// and [`RegistrationError::Persistence`] when the store rejects the
    /// write.
    pub async fn create(&self, payload: Value, context: &AdmittedContext) -> Result<Identifier, RegistrationError> {
        // Counted before the payload is expanded or a geometry built from it.
        admit_registration_geometries(&payload, self.geometry_limits)?;
        let id = match payload.get("id").and_then(Value::as_str) {
            Some(raw) => {
                let id = client_registration_id(raw)?;
                if self.repo.exists(&id).await? {
                    return Err(RegistrationError::already_exists(&id));
                }
                id
            }
            None => generate_registration_id()?,
        };

        // Clause 5.5.7: a context that does not process refuses the
        // registration before any of its names is expanded under it.
        self.engine.expansion_context(context).await?;

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
        let revision = self.repo.create(&id, &params).await?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the change event and response each own the identifier.
            trigger_reason: CsrTriggerReason::NewlyMatching,
            csr_snapshot: None,
            revision,
        });
        Ok(id)
    }
}
