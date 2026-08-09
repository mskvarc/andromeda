//! Stored lifecycle information needed for delete and reload.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use iri_rs::IriBuf;

/// Identity, kind and source of one stored Context.
#[derive(Debug)]
pub struct StoredContext {
    /// The local persistent identifier.
    pub identifier: Uri,
    /// How the Context entered the registry.
    pub kind: ContextKind,
    /// The remote or locally served public URL, when present.
    pub source_url: Option<IriBuf>,
}

#[cfg(test)]
mod tests {
    use crate::{error::ContextRegistryError, stored_context::StoredContext};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

    #[test]
    fn hosted_context_may_have_no_public_url() -> Result<(), ContextRegistryError> {
        let identifier = "urn:ngsi-ld:Context:local"
            .parse::<Uri>()
            .map_err(|error| ContextRegistryError::Invariant { detail: error.to_string() })?;
        let stored = StoredContext {
            identifier,
            kind: ContextKind::Hosted,
            source_url: None,
        };
        assert!(stored.source_url.is_none());
        Ok(())
    }
}
