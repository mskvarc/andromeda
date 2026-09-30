//! Recognising a reference to one of this broker's own stored `@context`s.
//!
//! Clause 5.13.1 serves Hosted and `ImplicitlyCreated` `@context`s "on-demand"
//! from the broker's storage, under the Serve operation of clause 6.30. A
//! request, or a context document, may name such a context by the URL the
//! broker minted for it. Fetching that URL over HTTP from the broker itself
//! would record a "Cached" copy of the broker's own document; the loader
//! resolves it from storage instead, and only for the tenant whose storage it
//! reads (clause 4.14 keeps each tenant's contexts to that tenant). A URL
//! shaped like one of the broker's own that names another tenant is never
//! recorded as a "Cached" copy either.
//!
//! Which references name a stored context is decided by the HTTP binding that
//! owns the clause 6.30 path, so this crate declares the port and the binding
//! implements it.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use iri_rs::IriRefBuf;
use std::fmt::Debug;

/// What a context reference is, as seen from one tenant's storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedReference {
    /// A stored `@context` of this tenant, named by its identifier.
    Own(JsonLdContextId),
    /// A stored `@context` of another tenant: shaped like one of the broker's
    /// own, but not this tenant's to read from storage.
    OtherTenant,
    /// Not a reference to a stored `@context` at all.
    Unrelated,
}

/// Recognises a reference to one of the tenant's own stored `@context`s.
pub trait HostedContextRecognizer: Debug + Send + Sync {
    /// What `reference`, an absolute or relative IRI reference, names.
    fn recognize(&self, reference: &IriRefBuf) -> HostedReference;
}

/// A recogniser that recognises nothing.
///
/// For an engine no broker tenant stands behind (a unit test, an offline
/// tool): every reference is resolved as the ordinary URL it is.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHostedContexts;

impl HostedContextRecognizer for NoHostedContexts {
    fn recognize(&self, _reference: &IriRefBuf) -> HostedReference {
        HostedReference::Unrelated
    }
}

#[cfg(test)]
mod tests {
    use crate::hosted_context_recognizer::{HostedContextRecognizer, HostedReference, NoHostedContexts};
    use iri_rs::IriRefBuf;

    #[test]
    fn a_recogniser_with_no_tenant_behind_it_recognises_nothing() {
        let reference = IriRefBuf::new("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:x".to_owned()).expect("a relative reference");
        pretty_assertions::assert_eq!(NoHostedContexts.recognize(&reference), HostedReference::Unrelated);
    }
}
