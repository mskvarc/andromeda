//! Context-source registration retrieval policy.

use crate::{
    RegistrationService,
    context_source_properties::compact_context_source_properties,
    error::RegistrationError,
    information_compaction::compact_information,
};
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;

impl RegistrationService {
    /// Retrieve one context-source registration by URI.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when no registration carries
    /// `id`, [`RegistrationError::Internal`] when the registration cannot be
    /// serialised, [`RegistrationError::JsonLd`] when `context` cannot be
    /// loaded or the response cannot be compacted under it, and
    /// [`RegistrationError::Persistence`] when the store rejects the read.
    pub async fn get(&self, id: &Identifier, context: &ContextSpec) -> Result<Value, RegistrationError> {
        let mut registration = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let additional_properties = registration.additional_properties.take();
        let mut value = serde_json::to_value(registration).map_err(|error| RegistrationError::internal(format!("Failed to serialize CSR: {error}")))?;
        let names = NameCompactor::new(&self.engine, context);
        if let Some(information) = value.get_mut("information") {
            compact_information(&names, information).await;
        }
        if let Some(properties) = additional_properties
            && let Some(compacted) = compact_context_source_properties(&names, properties).await?
            && let (Some(target), Some(source)) = (value.as_object_mut(), compacted.as_object())
        {
            target.extend(source.iter().map(|(key, member)| (key.clone(), member.clone()))); // clone: the response object owns compacted members after the temporary document is dropped.
        }
        Ok(value)
    }
}
