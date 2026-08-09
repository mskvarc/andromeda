//! Context-source registration retrieval.

use crate::{error::AndromedaError, service::csource_registration::CsourceRegistrationService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

impl CsourceRegistrationService {
    /// Retrieve one context-source registration by URI.
    pub async fn get(&self, id: &Uri, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let registration = self.repo.get(id).await?.ok_or_else(|| AndromedaError::csr_not_found(id.as_str()))?;
        let mut value = serde_json::to_value(registration).map_err(|error| AndromedaError::internal(format!("Failed to serialize CSR: {error}")))?;
        if let Some(information) = value.get_mut("information") {
            self.compact_information(information, context).await?;
        }
        Ok(value)
    }
}
