//! Context-source registration retrieval policy.

use crate::{RegistrationService, error::RegistrationError};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

impl RegistrationService {
    /// Retrieve one context-source registration by URI.
    pub async fn get(&self, id: &Uri, context: &ContextSpec) -> Result<Value, RegistrationError> {
        let registration = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let mut value = serde_json::to_value(registration).map_err(|error| RegistrationError::internal(format!("Failed to serialize CSR: {error}")))?;
        if let Some(information) = value.get_mut("information") {
            self.compact_information(information, context).await?;
        }
        Ok(value)
    }
}
