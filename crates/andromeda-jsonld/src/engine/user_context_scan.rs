//! Scanning a request-supplied `@context` for what clause 5.5.7 refuses.
//!
//! Clause 5.5.7 forbids a user `@context` to carry a JSON-LD Scoped Context,
//! anywhere it reaches. A context document is scanned once for the first term
//! whose definition carries a scoped `@context` and for the remote contexts it
//! references rather than declares, which a walk that can resolve them then
//! follows. The scan of a context named by URL is kept per URL, for as long as
//! the loader holds the document it was taken from.

use crate::{context_term::ContextTerm, core_context_precedence::IMPORT_KEYWORD, error::JsonLdError, loader::http::HttpContextLoader};
use dashmap::DashMap;
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;

/// What one request-supplied `@context` holds that clause 5.5.7 asks about.
#[derive(Debug, Clone, Default)]
pub(crate) struct UserContextScan {
    /// The first term found carrying a scoped context, if any.
    ///
    /// The clause 5.5.7 refusal in [`crate::user_context_admission`] names it
    /// in the problem detail, which is why the term is kept rather than a flag.
    pub(crate) scoped_context_term: Option<ContextTerm>,
    /// The remote contexts this document references rather than declares.
    ///
    /// Recorded so a walk that resolves contexts -- which this scan cannot,
    /// being synchronous and loader-less -- can follow them: a scoped context
    /// one hop away is as effective as one written inline.
    pub(crate) nested_references: Vec<IriBuf>,
}

/// Scanned request contexts, kept by context URL.
///
/// A scan is derived from the document the loader holds, so it is only as
/// fresh as that document: once the loader's entry lapses (clause 6.3.16 for a
/// download, the revalidation span for a document the broker stores itself)
/// and the document is read again, a scan of the old copy is stale. Each scan
/// therefore remembers the document it was taken from, and is reused only
/// while the loader still holds that same document.
#[derive(Debug, Default)]
pub(crate) struct UserContextScanCache {
    /// Scans indexed by their context IRI.
    scans: DashMap<IriBuf, HeldScan>,
}

/// One scan and the document it was taken from.
#[derive(Debug)]
struct HeldScan {
    /// The loader's document the scan was taken from, compared by identity.
    source: Arc<jsonld_syntax::Value>,
    /// The scan.
    scan: Arc<UserContextScan>,
}

impl UserContextScanCache {
    /// Resolve and scan a context URL, reusing the scan taken earlier while
    /// the loader still holds the document it was taken from.
    pub(crate) async fn get_or_load(&self, loader: &HttpContextLoader, url: &IriBuf) -> Result<Arc<UserContextScan>, JsonLdError> {
        let held = loader.held_document(url).await?;
        if let Some(held) = held.as_ref()
            && let Some(cached) = self.scans.get(url)
            && Arc::ptr_eq(&cached.source, held)
        {
            return Ok(Arc::clone(&cached.scan));
        }
        let value = loader.load_value(url).await?;
        let serde_value = jsonld_syntax::Value::into_serde_json((*value).clone()); // clone: the loader retains its cached document
        let scan = Arc::new(scan_user_context(&serde_value));
        self.scans.insert(
            url.clone(), // clone: the map owns its key while the caller keeps the URL
            HeldScan {
                source: value,
                scan: Arc::clone(&scan),
            },
        );
        Ok(scan)
    }

    /// Remove a scanned context after its source document changes.
    pub(crate) fn remove(&self, url: &IriBuf) {
        self.scans.remove(url);
    }

    /// The number of scanned URL contexts.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.scans.len()
    }
}

/// Scan one request-supplied `@context` value.
pub(crate) fn scan_user_context(value: &Value) -> UserContextScan {
    let mut scan = UserContextScan::default();
    scan_into(value, &mut scan);
    scan
}

/// Flatten a body `@context` while preserving JSON-LD declaration order.
pub(crate) fn collect_context_items<'a>(value: &'a Value, items: &mut Vec<&'a Value>) {
    match value {
        Value::Array(values) => values.iter().for_each(|value| collect_context_items(value, items)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => items.push(value),
    }
}

/// Recursively accumulate one JSON-LD context value into a scan.
fn scan_into(value: &Value, scan: &mut UserContextScan) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| scan_into(item, scan)),
        Value::Object(map) => {
            if let Some(inner) = map.get("@context") {
                scan_into(inner, scan);
                return;
            }
            // JSON-LD 1.1 section 4.1.10 merges an `@import`ed context into
            // this definition, scoped term definitions included, so the
            // document it names is followed like any other remote reference.
            if let Some(imported) = map.get(IMPORT_KEYWORD) {
                scan_into(imported, scan);
            }
            map.iter()
                .filter(|(key, _)| !key.starts_with('@'))
                .for_each(|(term, declaration)| scan_term_declaration(term, declaration, scan));
        }
        // A loaded context document may wrap its declarations in another
        // remote context URL (for example a suite context that imports the
        // test vocabulary and the core context); the reference is recorded so
        // the clause 5.5.7 walk can follow it.
        Value::String(reference) => {
            // owned: the scan outlives the borrowed context document
            if let Ok(url) = IriBuf::new(reference.to_owned()) {
                scan.nested_references.push(url);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Scan one term declaration for a scoped `@context`.
fn scan_term_declaration(term: &str, declaration: &Value, scan: &mut UserContextScan) {
    match declaration {
        Value::Object(definition) => {
            if definition.contains_key("@context") && scan.scoped_context_term.is_none() {
                scan.scoped_context_term = Some(ContextTerm::new(term));
            }
        }
        // JSON-LD 1.1 clause 4.1 gives a term definition a string or a map and
        // never an array, and the processor refuses one; its elements are still
        // searched, so a scoped `@context` written inside one is seen by the
        // clause 5.5.7 refusal rather than silently ignored.
        Value::Array(declarations) => declarations.iter().for_each(|declaration| scan_term_declaration(term, declaration, scan)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::ContextCache,
        context_registry::ContextRegistry,
        context_term::ContextTerm,
        engine::user_context_scan::{UserContextScanCache, collect_context_items, scan_user_context},
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference, NoHostedContexts},
        loader::{http::HttpContextLoader, loader_settings::LoaderSettings, replaceable_row::ReplaceableRow},
    };
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::json;
    use std::sync::Arc;

    /// A scan taken from a document is stale once the loader has read that
    /// document again (its in-process entry lapsed and the row changed), so the
    /// next lookup scans the document the loader holds now rather than
    /// returning the scan of the old one.
    #[tokio::test]
    async fn a_context_is_scanned_again_once_the_loader_reads_its_document_again() {
        let row = Arc::new(ReplaceableRow::new(json!({ "@context": { "speed": "https://example.org/speed" } })));
        let loader = HttpContextLoader::new(Arc::new(ContextRegistry::new()), LoaderSettings::offline(), Arc::new(NoHostedContexts))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>);
        let scans = UserContextScanCache::default();
        let url = IriBuf::new("urn:ngsi-ld:JsonLdContext:hosted".to_owned()).expect("a valid identifier");

        let first = scans.get_or_load(&loader, &url).await.expect("the first scan");
        let again = scans.get_or_load(&loader, &url).await.expect("the same scan");
        assert!(Arc::ptr_eq(&first, &again), "an unchanged document keeps its scan");
        assert!(first.scoped_context_term.is_none());

        row.replace(json!({ "@context": { "speed": {"@id": "https://example.org/speed", "@context": {}} } }));
        // The in-process entry lapsing is what the revalidation span does; the
        // scans are deliberately left alone here.
        loader.invalidate(&url).await;

        let rescanned = scans.get_or_load(&loader, &url).await.expect("the scan of the document read again");
        pretty_assertions::assert_eq!(rescanned.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// The identifier every reference below names.
    const OWN: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// Recognises the stored-context path of [`OWN`] on any origin.
    #[derive(Debug)]
    struct OwnPath;

    impl HostedContextRecognizer for OwnPath {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str().ends_with(&format!("/ngsi-ld/v1/jsonldContexts/{OWN}")) {
                HostedReference::Own(OWN.parse().expect("a stored identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    /// A stored `@context` named by its URL is held under its identifier, so
    /// the scan taken from it is reused while the loader still holds that
    /// document, like the scan of any other context.
    #[tokio::test]
    async fn the_scan_of_an_own_context_named_by_its_url_is_reused() {
        let row = Arc::new(ReplaceableRow::new(json!({ "@context": { "speed": "https://example.org/speed" } })));
        let loader = HttpContextLoader::new(Arc::new(ContextRegistry::new()), LoaderSettings::offline(), Arc::new(OwnPath))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>);
        let scans = UserContextScanCache::default();
        let url = IriBuf::new(format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{OWN}")).expect("a valid URL");

        let first = scans.get_or_load(&loader, &url).await.expect("the first scan");
        let again = scans.get_or_load(&loader, &url).await.expect("the same scan");

        assert!(Arc::ptr_eq(&first, &again), "an own context's scan is reused");
        pretty_assertions::assert_eq!(scans.len(), 1);
    }

    #[test]
    fn a_context_of_plain_terms_declares_no_scoped_context_and_names_no_nested_reference() {
        let scan = scan_user_context(&json!({"@context": {"speed": "https://example.org/speed"}}));
        assert!(scan.scoped_context_term.is_none(), "{:?}", scan.scoped_context_term);
        assert!(scan.nested_references.is_empty(), "{:?}", scan.nested_references);
    }

    /// The refusal of clause 5.5.7 names the offending term, so the scan keeps
    /// it rather than a flag.
    #[test]
    fn a_term_definition_carrying_a_scoped_context_records_the_term() {
        let scan = scan_user_context(&json!({
            "speed": {"@id": "https://example.org/speed", "@context": {"observedAt": "https://example.org/rewritten"}}
        }));
        pretty_assertions::assert_eq!(scan.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// A term whose definition carries a scoped `@context` but no `@id` still
    /// has its scoped context recorded.
    #[test]
    fn a_scoped_context_on_a_definition_without_an_id_is_still_recorded() {
        let scan = scan_user_context(&json!({"speed": {"@context": {}}}));
        pretty_assertions::assert_eq!(scan.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// A context document that names another remote context records the
    /// reference, so the clause 5.5.7 walk -- which can resolve it -- follows
    /// it, beside the sibling inline declarations.
    #[test]
    fn a_nested_remote_reference_is_recorded() {
        let scan = scan_user_context(&json!({"@context": [
            "https://example.org/other.jsonld",
            {"speed": {"@id": "https://example.org/speed", "@context": {}}}
        ]}));
        pretty_assertions::assert_eq!(
            scan.nested_references.iter().map(IriBuf::as_str).collect::<Vec<_>>(),
            vec!["https://example.org/other.jsonld"]
        );
        pretty_assertions::assert_eq!(scan.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// An `@import` merges the named context into the definition beside it
    /// (JSON-LD 1.1 section 4.1.10), so the reference is recorded for the
    /// clause 5.5.7 walk exactly as a bare remote reference is.
    #[test]
    fn an_import_is_recorded_as_a_nested_reference() {
        let scan = scan_user_context(&json!({"@import": "https://example.org/scoped.jsonld", "speed": "https://example.org/speed"}));
        pretty_assertions::assert_eq!(
            scan.nested_references.iter().map(IriBuf::as_str).collect::<Vec<_>>(),
            vec!["https://example.org/scoped.jsonld"]
        );
        assert!(scan.scoped_context_term.is_none(), "{:?}", scan.scoped_context_term);
    }

    /// The same `@import` inside a loaded document's `@context` wrapper.
    #[test]
    fn an_import_inside_a_context_wrapper_is_recorded() {
        let scan = scan_user_context(&json!({"@context": {"@import": "https://example.org/scoped.jsonld"}}));
        pretty_assertions::assert_eq!(
            scan.nested_references.iter().map(IriBuf::as_str).collect::<Vec<_>>(),
            vec!["https://example.org/scoped.jsonld"]
        );
    }

    /// A reference the IRI parser rejects names no document to resolve.
    #[test]
    fn a_nested_reference_that_is_not_an_iri_is_not_recorded() {
        let scan = scan_user_context(&json!({"@context": ["not an iri"]}));
        assert!(scan.nested_references.is_empty(), "{:?}", scan.nested_references);
    }

    /// JSON-LD 1.1 gives a term definition a string or a map and never an
    /// array; a scoped `@context` hidden inside one must still be seen.
    #[test]
    fn an_array_term_declaration_is_still_searched() {
        let scan = scan_user_context(&json!({"speed": [{"@id": "https://example.org/speed", "@context": {}}]}));
        pretty_assertions::assert_eq!(scan.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// The first scoped term found is the one reported; a second one does not
    /// replace it.
    #[test]
    fn the_first_scoped_term_found_is_the_one_reported() {
        let scan = scan_user_context(&json!({
            "alpha": {"@id": "https://example.org/alpha", "@context": {}},
            "beta": {"@id": "https://example.org/beta", "@context": {}}
        }));
        pretty_assertions::assert_eq!(scan.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("alpha"));
    }

    /// A keyword member of a context (`@vocab`, `@language`, `@version`)
    /// is no term definition and records nothing.
    #[test]
    fn a_keyword_member_is_no_term_definition() {
        let scan = scan_user_context(&json!({"@vocab": "https://example.org/", "@language": "en", "@version": 1.1}));
        assert!(scan.scoped_context_term.is_none(), "{:?}", scan.scoped_context_term);
        assert!(scan.nested_references.is_empty(), "{:?}", scan.nested_references);
    }

    /// Flattening a body context keeps every item in declaration order.
    #[test]
    fn a_body_context_flattens_in_declaration_order() {
        let body = json!(["https://example.org/a.jsonld", [{"speed": "https://example.org/speed"}, "https://example.org/b.jsonld"]]);
        let mut items = Vec::new();
        collect_context_items(&body, &mut items);
        pretty_assertions::assert_eq!(
            items,
            vec![
                &json!("https://example.org/a.jsonld"),
                &json!({"speed": "https://example.org/speed"}),
                &json!("https://example.org/b.jsonld")
            ]
        );
    }
}
