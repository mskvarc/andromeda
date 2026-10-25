//! Stored lifecycle information needed for delete and reload.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use iri_rs::IriBuf;

/// Identity, kind and source of one stored Context.
#[derive(Debug)]
pub struct StoredContext {
    /// The local persistent identifier.
    pub identifier: Identifier,
    /// How the Context entered the registry.
    pub kind: ContextKind,
    /// The remote or locally served public URL, when present.
    pub source_url: Option<IriBuf>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stored_context::StoredContext;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

    #[test]
    fn hosted_context_may_have_no_public_url() {
        let identifier = "urn:ngsi-ld:JsonLdContext:local".parse::<Identifier>().expect("a valid URI");
        let stored = StoredContext {
            identifier,
            kind: ContextKind::Hosted,
            source_url: None,
        };
        assert!(stored.source_url.is_none());
    }
}
