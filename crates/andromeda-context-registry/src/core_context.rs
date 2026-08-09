//! Embedded NGSI-LD core Context representation and metadata.

use crate::error::ContextRegistryError;
use andromeda_jsonld::core_context::{NGSI_LD_CORE_CONTEXT_V19, is_ngsi_ld_core_context_url};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
use chrono::Utc;
use iri_rs::Iri;
use serde_json::Value;

/// Whether an identifier names one of the embedded NGSI-LD core Context versions.
pub(crate) fn is_core_context(identifier: &Uri) -> bool {
    Iri::parse(identifier.as_str()).is_ok_and(is_ngsi_ld_core_context_url)
}

/// Parse the embedded v1.9 core Context document.
pub(crate) fn payload() -> Result<Value, ContextRegistryError> {
    serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).map_err(|error| ContextRegistryError::Invariant {
        detail: format!("embedded core Context JSON is invalid: {error}"),
    })
}

/// Build the virtual metadata record for an embedded core Context.
pub(crate) fn metadata(identifier: &Uri) -> Result<ContextMetadata, ContextRegistryError> {
    let Uri::Url(url) = identifier else {
        return Err(ContextRegistryError::Invariant {
            detail: "core Context identifier is not a URL".to_string(),
        });
    };
    let url = url.clone(); // clone: metadata owns both the public URL and its URI identifier.
    let local_id = Uri::Url(url.clone()); // clone: the URL is also retained in the metadata URL field.
    let now = Utc::now();
    Ok(ContextMetadata::builder()
        .url(Some(url))
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
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

    fn identifier(value: &str) -> Result<Uri, ContextRegistryError> {
        value
            .parse::<Uri>()
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
