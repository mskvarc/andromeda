//! Which `@context` documents the broker resolves from memory.
//!
//! Every released NGSI-LD core context (clause 4.4) is embedded, and so is the
//! one document the v1.9 Core @context names itself: the W3C Data Integrity v2
//! context, the scoped context of the `ngsildproof` term (clause 4.5.2.2).
//! Embedding both means the Core @context expands completely without a
//! network, so a proof member written with a W3C term expands to the same IRI
//! whether or not the broker can reach the W3C.
//!
//! An embedded document is resolved from memory and never fetched. It is no
//! clause 5.13 entry: the @context management operations neither list nor
//! serve it, the durable context cache keeps no record of it, and an operator
//! cannot preload a replacement for it.

use crate::core_context::{embedded_core_context, is_ngsi_ld_core_context_url};
use andromeda_ngsi_ld_vocabulary::{
    core_context::NGSI_LD_CORE_CONTEXT_RELEASES,
    core_context_release::CoreContextRelease,
    data_integrity_context::{DATA_INTEGRITY_CONTEXT, DATA_INTEGRITY_CONTEXT_URL},
};
use iri_rs::Iri;

/// Whether `url` names a document compiled into the broker.
#[must_use]
pub fn is_embedded_context_url(url: Iri<&str>) -> bool {
    is_ngsi_ld_core_context_url(url) || url == DATA_INTEGRITY_CONTEXT_URL
}

/// The document compiled into the broker under `url`, if there is one.
#[must_use]
pub fn embedded_context(url: Iri<&str>) -> Option<&'static str> {
    embedded_core_context(url).or_else(|| (url == DATA_INTEGRITY_CONTEXT_URL).then_some(DATA_INTEGRITY_CONTEXT))
}

/// Every URL a document is compiled into the broker under.
pub fn embedded_context_urls() -> impl Iterator<Item = Iri<&'static str>> {
    NGSI_LD_CORE_CONTEXT_RELEASES
        .iter()
        .map(CoreContextRelease::url)
        .chain([DATA_INTEGRITY_CONTEXT_URL])
}

#[cfg(test)]
mod tests {
    use crate::embedded_context::{embedded_context, embedded_context_urls, is_embedded_context_url};
    use andromeda_ngsi_ld_vocabulary::{
        core_context::NGSI_LD_CORE_CONTEXT_RELEASES,
        data_integrity_context::{DATA_INTEGRITY_CONTEXT, DATA_INTEGRITY_CONTEXT_URL},
    };
    use iri_rs::iri;

    #[test]
    fn the_data_integrity_url_resolves_to_the_data_integrity_document() {
        pretty_assertions::assert_eq!(embedded_context(DATA_INTEGRITY_CONTEXT_URL), Some(DATA_INTEGRITY_CONTEXT));
    }

    #[test]
    fn every_core_context_and_the_data_integrity_context_are_embedded() {
        for url in NGSI_LD_CORE_CONTEXT_RELEASES
            .map(|release| release.url())
            .into_iter()
            .chain([DATA_INTEGRITY_CONTEXT_URL])
        {
            assert!(is_embedded_context_url(url), "{url} is embedded");
            assert!(embedded_context(url).is_some(), "{url} has a payload");
            assert!(embedded_context_urls().any(|embedded| embedded == url), "{url} is registered");
        }
    }

    #[test]
    fn a_context_the_broker_does_not_compile_in_is_not_embedded() {
        let url = iri!("https://w3id.org/security/multikey/v1");
        assert!(!is_embedded_context_url(url));
        assert!(embedded_context(url).is_none());
    }
}
