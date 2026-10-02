//! Reading and rewriting the URLs a document's `@context` names.

use crate::{
    core_context::is_ngsi_ld_core_context_url,
    hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
};
use iri_rs::{IriBuf, IriRefBuf, iri};
use serde_json::Value;
use std::collections::HashSet;

/// Whether a term is already an absolute IRI and needs no expansion.
///
/// A conservative short-circuit for the common schemes; a term this rejects is
/// not necessarily a plain term (see [`is_absolute_iri`]), so callers that must
/// distinguish an absolute IRI from a compact one only after ruling out a
/// defined prefix use that function instead.
///
/// It is also the rule deciding which names a name compaction compacts at
/// all: a name outside these schemes is answered verbatim.
#[must_use]
pub fn is_full_iri(term: &str) -> bool {
    term.starts_with("http://") || term.starts_with("https://") || term.starts_with("urn:")
}

/// Whether a term has the form of an absolute IRI: a scheme (RFC 3987
/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) followed by `:`.
///
/// Unlike [`is_full_iri`] this admits every scheme — `did:`, `mailto:`, `tel:` —
/// so a context reference written with any scheme is kept as the absolute
/// IRI it is rather than resolved against a base (JSON-LD 1.1 clause 4.6.2).
/// `did:example:1` is the case the three-scheme allowlist mis-read.
pub(crate) fn is_absolute_iri(term: &str) -> bool {
    let mut chars = term.char_indices();
    match chars.next() {
        Some((_, first)) if first.is_ascii_alphabetic() => {}
        Some(_) | None => return false,
    }
    for (index, ch) in chars {
        if ch == ':' {
            return index > 0;
        }
        if !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.') {
            return false;
        }
    }
    false
}

/// Collect every context URL a `@context` node names.
///
/// Used on the failure path, once the document itself has been consumed by
/// the processor and only its context survives for the diagnostic.
pub(crate) fn context_urls_of(context: &Value) -> Vec<IriBuf> {
    let mut urls = Vec::new();
    collect_context_urls(context, &mut urls);
    urls
}

/// Walk a `@context` node accumulating the URLs it names.
fn collect_context_urls(value: &Value, urls: &mut Vec<IriBuf>) {
    match value {
        Value::String(url) => {
            if let Ok(url) = IriBuf::new(url.clone()) {
                urls.push(url);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_context_urls(item, urls);
            }
        }
        Value::Object(map) => {
            if let Some(context) = map.get("@context") {
                collect_context_urls(context, urls);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Collapse core-context aliases and repeats for a log line.
///
/// A diagnostic that lists v1.6 and the versionless URL as two separate
/// contexts sends the reader looking for a difference that is not there.
pub(crate) fn canonical_context_urls(urls: &[IriBuf]) -> Vec<IriBuf> {
    let mut seen = HashSet::new();
    let mut canonical = Vec::with_capacity(urls.len());
    for url in urls {
        let candidate = if is_ngsi_ld_core_context_url(url.as_ref()) {
            IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"))
        } else {
            url.clone() // clone: the slice is borrowed and stays valid
        };
        if seen.insert(candidate.clone()) {
            // clone: the set keeps a copy, the list keeps a copy
            canonical.push(candidate);
        }
    }
    canonical
}

/// Rewrite root-relative references to the tenant's own stored `@context`s to
/// their identifier.
///
/// A client that read a context from this broker may echo the path it read it
/// from as its `@context`. A request body has no base URL to resolve such a
/// reference against, so the processor could not dereference it; the
/// identifier is a URN, which the loader resolves from storage (clause 5.13.1
/// serves the document "on-demand"). An absolute reference is left as it is:
/// the loader recognises it when it is resolved, and resolves it from storage
/// only when the tenant holds the identifier it names. Which references name a
/// stored context is `hosted`'s to say.
pub(crate) fn resolve_relative_context_urls(document: &mut Value, hosted: &dyn HostedContextRecognizer) {
    match document {
        Value::Object(object) => {
            if let Some(context) = object.get_mut("@context") {
                resolve_hosted_entry(context, hosted);
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(object) = item
                    && let Some(context) = object.get_mut("@context")
                {
                    resolve_hosted_entry(context, hosted);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Rewrite one `@context` entry, or each member of an array of them.
///
/// A relative reference that is no IRI reference at all, or that names no
/// stored `@context` of the tenant, is left as written for the processor to
/// refuse or resolve.
fn resolve_hosted_entry(value: &mut Value, hosted: &dyn HostedContextRecognizer) {
    match value {
        Value::String(url) if !is_absolute_iri(url) => {
            if let Ok(reference) = IriRefBuf::new(url.as_str())
                && let HostedReference::Own(identifier) = hosted.recognize(&reference)
            {
                *url = identifier.to_string();
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve_hosted_entry(item, hosted);
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core_context::{NGSI_LD_CORE_CONTEXT_UNVERSIONED_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
        engine::context_url::{canonical_context_urls, context_urls_of, is_absolute_iri, is_full_iri, resolve_relative_context_urls},
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::json;

    #[test]
    fn a_term_counts_as_a_full_iri_only_under_http_https_or_urn() {
        assert!(is_full_iri("https://example.com/Type"));
        assert!(is_full_iri("http://example.com/Type"));
        assert!(is_full_iri("urn:ngsi-ld:Type"));
        assert!(!is_full_iri("Vehicle"));
        assert!(!is_full_iri("ex:Vehicle"));
        // A term that merely starts with `http` but carries no scheme is not a
        // full IRI. Compaction's own gate is this predicate, so an attribute
        // named `httpEndpoint` is no longer mistaken for one it could shorten.
        assert!(!is_full_iri("httpEndpoint"));
        // A made-up scheme is not one of the three the short-circuit admits.
        assert!(!is_full_iri("httpfoo:x"));
    }

    #[test]
    fn a_string_with_any_scheme_has_the_form_of_an_absolute_iri() {
        // Clause 4.6.2: any scheme, not just the three the fast short-circuit lists.
        assert!(is_absolute_iri("https://example.com/Type"));
        assert!(is_absolute_iri("urn:ngsi-ld:Type"));
        assert!(is_absolute_iri("did:example:123"));
        assert!(is_absolute_iri("mailto:x@example.com"));
        // `ex:Vehicle` has IRI form; whether it expands as a compact IRI is
        // decided by the prefix table before this is consulted.
        assert!(is_absolute_iri("ex:Vehicle"));
        // A plain term has no scheme.
        assert!(!is_absolute_iri("Vehicle"));
        // A leading digit is not a valid scheme start.
        assert!(!is_absolute_iri("1did:x"));
    }

    #[test]
    fn every_url_a_nested_context_names_is_collected() {
        let context = json!(["https://example.com/a", { "@context": ["https://example.com/b"] }, { "term": "https://example.com/t" }]);
        pretty_assertions::assert_eq!(
            context_urls_of(&context),
            vec![
                IriBuf::new("https://example.com/a".to_string()).expect("valid URL"),
                IriBuf::new("https://example.com/b".to_string()).expect("valid URL"),
            ]
        );
    }

    #[test]
    fn core_context_aliases_collapse_to_one_canonical_url_for_a_diagnostic() {
        let urls = vec![
            IriBuf::new(NGSI_LD_CORE_CONTEXT_V16_URL.to_string()).expect("valid URL"),
            IriBuf::new(NGSI_LD_CORE_CONTEXT_URL.to_string()).expect("valid URL"),
            IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL"),
        ];
        pretty_assertions::assert_eq!(
            canonical_context_urls(&urls),
            vec![
                IriBuf::new(NGSI_LD_CORE_CONTEXT_UNVERSIONED_URL.to_string()).expect("valid URL"),
                IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL"),
            ]
        );
    }

    /// Recognises the one hosted path the test stores.
    #[derive(Debug)]
    struct OneHostedContext;

    impl HostedContextRecognizer for OneHostedContext {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str() == "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc" {
                HostedReference::Own("urn:ngsi-ld:JsonLdContext:abc".parse().expect("an identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    #[test]
    fn a_reference_to_a_stored_context_is_rewritten_to_its_identifier() {
        let mut document = json!({
            "@context": ["/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc", "https://example.com/keep"],
            "id": "urn:ngsi-ld:Device:1"
        });

        resolve_relative_context_urls(&mut document, &OneHostedContext);

        pretty_assertions::assert_eq!(document["@context"], json!(["urn:ngsi-ld:JsonLdContext:abc", "https://example.com/keep"]));
    }

    /// Recognises the stored path on any origin.
    #[derive(Debug)]
    struct HostedOnAnyOrigin;

    impl HostedContextRecognizer for HostedOnAnyOrigin {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str().ends_with("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc") {
                HostedReference::Own("urn:ngsi-ld:JsonLdContext:abc".parse().expect("an identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    /// An absolute reference is the loader's to resolve: it serves it from
    /// storage only when the tenant holds the identifier, and otherwise as the
    /// URL it is, which a rewrite to the identifier would make impossible.
    #[test]
    fn an_absolute_reference_to_a_stored_context_is_left_for_the_loader() {
        let absolute = "http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:abc";
        let mut document = json!({ "@context": absolute, "id": "urn:ngsi-ld:Device:1" });

        resolve_relative_context_urls(&mut document, &HostedOnAnyOrigin);

        pretty_assertions::assert_eq!(document["@context"], json!(absolute));
    }
}
