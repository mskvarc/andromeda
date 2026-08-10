//! Context-source registration retrieval policy.

use crate::{RegistrationService, context_source_properties::compact_context_source_properties, error::RegistrationError};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

impl RegistrationService {
    /// Retrieve one context-source registration by URI.
    pub async fn get(&self, id: &Uri, context: &ContextSpec) -> Result<Value, RegistrationError> {
        let mut registration = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let additional_properties = registration.additional_properties.take();
        let mut value = serde_json::to_value(registration).map_err(|error| RegistrationError::internal(format!("Failed to serialize CSR: {error}")))?;
        if let Some(information) = value.get_mut("information") {
            self.compact_information(information, context).await?;
        }
        if let Some(properties) = additional_properties
            && let Some(compacted) = compact_context_source_properties(&self.engine, properties, context).await?
            && let (Some(target), Some(source)) = (value.as_object_mut(), compacted.as_object())
        {
            target.extend(source.iter().map(|(key, member)| (key.clone(), member.clone()))); // clone: the response object owns compacted members after the temporary document is dropped.
        }
        Ok(value)
    }
}
