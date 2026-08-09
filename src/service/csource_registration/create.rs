//! Context-source registration creation.

use crate::{
    error::AndromedaError,
    event::csr_change_event::CsrChangeEvent,
    service::csource_registration::{
        CsourceRegistrationService,
        identifier::generate_registration_id,
        payload::extract_create_params,
        validation::validate_common_members,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::CsrTriggerReason;
use serde_json::Value;

impl CsourceRegistrationService {
    /// Create a context-source registration from a JSON-LD payload.
    pub async fn create(&self, payload: Value, context: &ContextSpec) -> Result<Uri, AndromedaError> {
        validate_common_members(&payload)?;
        if payload.get("information").is_none() {
            return Err(AndromedaError::csr_validation("information is required"));
        }
        if payload.get("endpoint").is_none() {
            return Err(AndromedaError::csr_validation("endpoint is required"));
        }

        let id = match payload.get("id").and_then(Value::as_str) {
            Some(raw) => {
                let id = raw.parse::<Uri>().map_err(|_| AndromedaError::csr_validation("id must be a valid URI"))?;
                if self.repo.exists(&id).await? {
                    return Err(AndromedaError::csr_already_exists(id.as_str()));
                }
                id
            }
            None => generate_registration_id()?,
        };

        let validation_document = serde_json::json!({
            "id": "urn:ngsi-ld:_:context-validate",
            "type": "ContextSourceRegistration"
        });
        self.engine.expand(validation_document, context).await?;

        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let params = extract_create_params(&payload)?;
        self.repo.create(&id, &params).await?;
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: the change event and response each own the identifier.
            trigger_reason: CsrTriggerReason::NewlyMatching,
            csr_snapshot: None,
        });
        Ok(id)
    }
}
