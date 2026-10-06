//! Context-source registration retrieval policy.

use crate::{RegistrationService, error::RegistrationError, registration_compaction::compacted_registration};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

impl RegistrationService {
    /// Retrieve one context-source registration by URI, its names compacted
    /// under `context`.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when no registration carries
    /// `id`, [`RegistrationError::JsonLd`] when `context` cannot be loaded or
    /// the response cannot be compacted under it, and
    /// [`RegistrationError::Persistence`] when the store rejects the read.
    pub async fn get(&self, id: &Identifier, context: &ContextSpec) -> Result<ContextSourceRegistration, RegistrationError> {
        let registration = self.repo.get(id).await?.ok_or_else(|| RegistrationError::not_found(id))?;
        let names = NameCompaction::new(self.engine.compactor(context).processed_context().await?);
        Ok(compacted_registration(&names, registration))
    }
}
