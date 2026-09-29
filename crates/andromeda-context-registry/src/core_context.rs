//! Embedded NGSI-LD core Context representation and metadata.

use crate::error::ContextRegistryError;
use andromeda_jsonld::core_context::{NGSI_LD_CORE_CONTEXT_V19, is_ngsi_ld_core_context_url};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
use chrono::Utc;
use iri_rs::{Iri, IriBuf};
use serde_json::Value;

/// Whether an identifier names one of the embedded NGSI-LD core Context versions.
pub(crate) fn is_core_context(identifier: &Identifier) -> bool {
    Iri::parse(identifier.as_str()).is_ok_and(is_ngsi_ld_core_context_url)
}

/// Parse the embedded v1.9 core Context document.
pub(crate) fn payload() -> Result<Value, ContextRegistryError> {
    serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).map_err(|error| ContextRegistryError::Invariant {
        detail: format!("embedded core Context JSON is invalid: {error}"),
    })
}

/// Build the virtual metadata record for an embedded core Context.
pub(crate) fn metadata(identifier: &Identifier) -> Result<ContextMetadata, ContextRegistryError> {
    let Some(url) = identifier.as_url() else {
        return Err(ContextRegistryError::Invariant {
            detail: "core Context identifier is not a URL".to_string(),
        });
    };
    let url = url.clone(); // clone: metadata owns both the public URL and its URI identifier.
    let local_id = identifier.clone(); // clone: the identifier is also retained as the metadata's local id.
    // The embedded core Context serves under its own URL; keep it verbatim as
    // the hit-counter key, the same string a `numberOfHits` fold looks up.
    let serving_iri = IriBuf::new(url.as_str().to_owned()).map_err(|error| ContextRegistryError::Invariant {
        detail: format!("core Context URL is not an IRI reference: {}", error.0),
    })?;
    let now = Utc::now();
    Ok(ContextMetadata::builder()
        .url(Some(url))
        .serving_iri(Some(serving_iri))
        .local_id(local_id)
        .kind(ContextKind::ImplicitlyCreated)
        .created_at(now)
        .expires_at(None)
        .last_usage(now)
        .number_of_hits(0)
        .extra_info(None)
        .build())
}

#[cfg(test)]
mod tests {
    use crate::{
        core_context::{is_core_context, metadata, payload},
        error::ContextRegistryError,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

    fn identifier(value: &str) -> Result<Identifier, ContextRegistryError> {
        value
            .parse::<Identifier>()
            .map_err(|error| ContextRegistryError::Invariant { detail: error.to_string() })
    }

    #[test]
    fn embedded_v19_context_is_recognized_and_representable() -> Result<(), ContextRegistryError> {
        let identifier = identifier("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld")?;
        assert!(is_core_context(&identifier));
        assert!(payload()?.get("@context").is_some());
        pretty_assertions::assert_eq!(metadata(&identifier)?.kind, ContextKind::ImplicitlyCreated);
        Ok(())
    }
}
