//! Parsing and caching of request-supplied JSON-LD term declarations.

use crate::{
    context_term::ContextTerm,
    engine::{
        core_term_semantics::{coercion_of, container_of},
        term_map::{
            coercion::{TermCoercion, TermContainer},
            declared_iri::DeclaredIri,
        },
    },
    error::JsonLdError,
    loader::http::HttpContextLoader,
};
use dashmap::DashMap;
use iri_rs::IriBuf;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// One JSON-LD term declaration, exactly as a context document states it.
///
/// The IRI is held as the text the document wrote rather than as a validated
/// [`IriBuf`], because clause 4.1.2 resolves it against the active context and
/// a compact IRI such as `example:Place` parses as an absolute one. Resolution
/// belongs to [`crate::engine::term_map::declared_iri`], which runs once the
/// context this declaration came from is merged.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredTerm {
    /// Term IRI as the declaration wrote it.
    pub(crate) iri: DeclaredIri,
    /// Declared type coercion.
    pub(crate) coercion: Option<TermCoercion>,
    /// Declared value container.
    pub(crate) container: Option<TermContainer>,
    /// Whether the term carries a scoped context.
    pub(crate) has_scoped_context: bool,
    /// Whether this is a simple string declaration usable as a prefix.
    pub(crate) simple: bool,
}

/// Request context information understood by the batch fast path.
#[derive(Debug, Clone)]
pub(crate) struct UserTermTable {
    /// Terms declared by the context, before the active context resolves them.
    pub(crate) terms: HashMap<Box<str>, DeclaredTerm>,
    /// The first term found carrying a scoped context, if any.
    ///
    /// The fastpath only needs to know that one exists; the clause 5.5.7
    /// refusal in [`crate::scoped_context_admission`] names it in the problem
    /// detail, which is why the term is kept rather than a flag.
    pub(crate) scoped_context_term: Option<ContextTerm>,
    /// Whether the context uses unsupported JSON-LD behavior.
    pub(crate) has_unsupported_feature: bool,
    /// The remote contexts this document references rather than declares.
    ///
    /// Recorded so a walk that resolves contexts -- which this parse cannot,
    /// being synchronous and loader-less -- can follow them; the fastpath
    /// itself demotes on one through [`UserTermTable::has_unsupported_feature`].
    pub(crate) nested_references: Vec<IriBuf>,
}

/// Parsed request contexts cached by context URL.
///
/// A table is a copy derived from the document the loader holds, so it is only
/// as fresh as that document: once the loader's entry lapses (clause 6.3.16
/// for a download, the revalidation span for a document the broker stores
/// itself) and the document is read again, a table parsed from the old copy is
/// stale. Each table therefore remembers the document it was parsed from, and
/// is reused only while the loader still holds that same document.
#[derive(Debug, Default)]
pub(crate) struct UserTermTableCache {
    /// Parsed term tables indexed by their context IRI.
    tables: DashMap<IriBuf, ParsedTermTable>,
}

/// One parsed table and the document it was parsed from.
#[derive(Debug)]
struct ParsedTermTable {
    /// The loader's document the table was parsed from, compared by identity.
    source: Arc<jsonld_syntax::Value>,
    /// The parsed table.
    table: Arc<UserTermTable>,
}

impl UserTermTableCache {
    /// Resolve and parse a context URL, reusing the table parsed earlier while
    /// the loader still holds the document it was parsed from.
    pub(crate) async fn get_or_load(&self, loader: &HttpContextLoader, url: &IriBuf) -> Result<Arc<UserTermTable>, JsonLdError> {
        let held = loader.held_document(url).await?;
        if let Some(held) = held.as_ref()
            && let Some(cached) = self.tables.get(url)
            && Arc::ptr_eq(&cached.source, held)
        {
            return Ok(Arc::clone(&cached.table));
        }
        let value = loader.load_value(url).await?;
        let serde_value = jsonld_syntax::Value::into_serde_json((*value).clone()); // clone: the loader retains its cached document
        let table = Arc::new(parse_user_context(&serde_value));
        self.tables.insert(
            url.clone(), // clone: the map owns its key while the caller keeps the URL
            ParsedTermTable {
                source: value,
                table: Arc::clone(&table),
            },
        );
        Ok(table)
    }

    /// Remove a parsed context after its source document changes.
    pub(crate) fn remove(&self, url: &IriBuf) {
        self.tables.remove(url);
    }

    /// Return the number of parsed URL contexts.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.tables.len()
    }
}

/// Parse one request-supplied `@context` value.
pub(crate) fn parse_user_context(value: &Value) -> UserTermTable {
    let mut table = UserTermTable {
        terms: HashMap::new(),
        scoped_context_term: None,
        has_unsupported_feature: false,
        nested_references: Vec::new(),
    };
    parse_user_context_into(value, &mut table);
    table
}

/// Recursively accumulate one JSON-LD context value into a term table.
fn parse_user_context_into(value: &Value, table: &mut UserTermTable) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| parse_user_context_into(item, table)),
        Value::Object(map) => {
            if let Some(inner) = map.get("@context") {
                parse_user_context_into(inner, table);
                return;
            }
            map.iter().for_each(|(key, declaration)| match key.as_str() {
                "@vocab" => {}
                other if other.starts_with('@') => table.has_unsupported_feature = true,
                _term => parse_term_declaration(key, declaration, table),
            });
        }
        // A loaded context document may wrap its declarations in another
        // remote context URL (for example a suite context that imports the
        // test vocabulary and the core context). The fast path has no base
        // URL or recursive loader here, so treating that URL as an empty table
        // silently expands the entity's terms under the default vocabulary.
        // Demote such documents to the full JSON-LD processor instead of
        // producing a different expansion. The reference is recorded even so:
        // clause 5.5.7's refusal of a scoped context has to follow it, or a
        // scoped context one hop away is invisible.
        Value::String(reference) => {
            table.has_unsupported_feature = true;
            // owned: the parsed table outlives the borrowed context document
            if let Ok(url) = IriBuf::new(reference.to_owned()) {
                table.nested_references.push(url);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Parse one compact-term declaration into its fast-path representation.
fn parse_term_declaration(term: &str, declaration: &Value, table: &mut UserTermTable) {
    match declaration {
        Value::String(iri) => {
            table.terms.insert(
                Box::<str>::from(term),
                DeclaredTerm {
                    iri: DeclaredIri::new(iri),
                    coercion: None,
                    container: None,
                    has_scoped_context: false,
                    simple: true,
                },
            );
        }
        Value::Object(definition) => {
            const MODELED: &[&str] = &["@id", "@type", "@container", "@context"];
            if definition.keys().any(|key| !MODELED.contains(&key.as_str())) {
                table.has_unsupported_feature = true;
            }
            let has_scoped_context = definition.contains_key("@context");
            if has_scoped_context && table.scoped_context_term.is_none() {
                table.scoped_context_term = Some(ContextTerm::new(term));
            }
            let Some(iri) = definition.get("@id").and_then(Value::as_str) else {
                return;
            };
            table.terms.insert(
                Box::<str>::from(term),
                DeclaredTerm {
                    iri: DeclaredIri::new(iri),
                    coercion: declared_coercion(definition.get("@type").and_then(Value::as_str)),
                    container: container_of(definition.get("@container").and_then(Value::as_str)),
                    has_scoped_context,
                    simple: false,
                },
            );
        }
        // JSON-LD 1.1 clause 4.1 gives a term definition a string or a map and
        // never an array, so the processor raises an invalid-term-definition
        // error for one and the fastpath must not pre-empt that with an
        // expansion of its own. The elements are still walked, so a scoped
        // `@context` written inside one is seen by the clause 5.5.7 refusal
        // rather than silently ignored.
        Value::Array(declarations) => {
            table.has_unsupported_feature = true;
            for declaration in declarations {
                parse_term_declaration(term, declaration, table);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Resolve one declared JSON-LD type coercion.
fn declared_coercion(declared_type: Option<&str>) -> Option<TermCoercion> {
    coercion_of(declared_type).or_else(|| declared_type.and_then(iri_from_context).map(TermCoercion::TypeIri))
}

/// Validate an IRI supplied by a context declaration.
fn iri_from_context(value: &str) -> Option<IriBuf> {
    IriBuf::new(value.to_owned()).ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::{CachedContext, ContextCache, ContextCacheError, StoredDocumentSource},
        context_registry::ContextRegistry,
        context_term::ContextTerm,
        engine::term_map::user_term::{UserTermTableCache, parse_user_context},
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference, NoHostedContexts},
        loader::{
            cache_lifetime::{CacheLifetime, ResidualLifetime},
            http::HttpContextLoader,
            loader_settings::LoaderSettings,
        },
    };
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::{Value, json};
    use std::sync::{Arc, RwLock};

    /// A durable cache holding one broker-stored row whose document a test can
    /// replace, as another broker process sharing the database could.
    #[derive(Debug)]
    struct ReplaceableRow {
        /// The document the row holds now.
        payload: RwLock<Value>,
    }

    #[async_trait::async_trait]
    impl ContextCache for ReplaceableRow {
        async fn get(&self, _url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            Ok(Some(CachedContext {
                payload: self.payload.read().expect("the row lock").clone(), // clone: the row keeps its document for the next read
                residual: ResidualLifetime::Unbounded,
                canonical_url: None,
                source: StoredDocumentSource::Broker,
            }))
        }

        async fn put(&self, _url: &IriBuf, _document_url: &IriBuf, _payload: &Value, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            Ok(())
        }
    }

    /// A table parsed from a document is stale once the loader has read that
    /// document again (its in-process entry lapsed and the row changed), so the
    /// next lookup parses the document the loader holds now rather than
    /// returning the table parsed from the old one.
    #[tokio::test]
    async fn a_table_is_parsed_again_once_the_loader_reads_its_document_again() {
        let row = Arc::new(ReplaceableRow {
            payload: RwLock::new(json!({ "@context": { "speed": "https://example.org/speed" } })),
        });
        let loader = HttpContextLoader::new(Arc::new(ContextRegistry::new()), LoaderSettings::offline(), Arc::new(NoHostedContexts))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>);
        let tables = UserTermTableCache::default();
        let url = IriBuf::new("urn:ngsi-ld:JsonLdContext:hosted".to_owned()).expect("a valid identifier");

        let first = tables.get_or_load(&loader, &url).await.expect("the first table");
        let again = tables.get_or_load(&loader, &url).await.expect("the same table");
        assert!(Arc::ptr_eq(&first, &again), "an unchanged document keeps its parsed table");

        *row.payload.write().expect("the row lock") = json!({ "@context": { "velocity": "https://example.org/velocity" } });
        // The in-process entry lapsing is what the revalidation span does; the
        // parsed tables are deliberately left alone here.
        loader.invalidate(&url).await;

        let reread = tables.get_or_load(&loader, &url).await.expect("the table of the document read again");
        assert!(reread.terms.contains_key("velocity"), "{:?}", reread.terms.keys().collect::<Vec<_>>());
        assert!(!reread.terms.contains_key("speed"));
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
    /// the table parsed from it is reused while the loader still holds that
    /// document, like the table of any other context.
    #[tokio::test]
    async fn the_table_of_an_own_context_named_by_its_url_is_reused() {
        let row = Arc::new(ReplaceableRow {
            payload: RwLock::new(json!({ "@context": { "speed": "https://example.org/speed" } })),
        });
        let loader = HttpContextLoader::new(Arc::new(ContextRegistry::new()), LoaderSettings::offline(), Arc::new(OwnPath))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>);
        let tables = UserTermTableCache::default();
        let url = IriBuf::new(format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{OWN}")).expect("a valid URL");

        let first = tables.get_or_load(&loader, &url).await.expect("the first table");
        let again = tables.get_or_load(&loader, &url).await.expect("the same table");

        assert!(Arc::ptr_eq(&first, &again), "an own context's parsed table is reused");
    }

    #[test]
    fn a_context_of_plain_terms_declares_no_scoped_context_and_names_no_nested_reference() {
        let table = parse_user_context(&json!({"@context": {"speed": "https://example.org/speed"}}));
        assert!(table.scoped_context_term.is_none(), "{:?}", table.scoped_context_term);
        assert!(table.nested_references.is_empty(), "{:?}", table.nested_references);
        assert!(!table.has_unsupported_feature);
        assert!(table.terms.contains_key("speed"));
    }

    /// The refusal of clause 5.5.7 names the offending term, so the parse keeps
    /// it rather than a flag.
    #[test]
    fn a_term_definition_carrying_a_scoped_context_records_the_term() {
        let table = parse_user_context(&json!({
            "speed": {"@id": "https://example.org/speed", "@context": {"observedAt": "https://example.org/rewritten"}}
        }));
        pretty_assertions::assert_eq!(table.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// A term whose definition carries a scoped `@context` but no `@id` is not
    /// added to the term table, and its scoped context is still recorded.
    #[test]
    fn a_scoped_context_on_a_definition_without_an_id_is_still_recorded() {
        let table = parse_user_context(&json!({"speed": {"@context": {}}}));
        pretty_assertions::assert_eq!(table.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
        assert!(!table.terms.contains_key("speed"));
    }

    /// A context document that names another remote context declares nothing
    /// itself. The fastpath cannot resolve it and demotes, and the reference is
    /// recorded so the clause 5.5.7 walk -- which can resolve it -- follows it.
    #[test]
    fn a_nested_remote_reference_is_recorded_and_demotes_the_fastpath() {
        let table = parse_user_context(&json!({"@context": ["https://example.org/other.jsonld", {"speed": "https://example.org/speed"}]}));
        assert!(table.has_unsupported_feature, "the fastpath cannot follow a nested reference");
        pretty_assertions::assert_eq!(
            table.nested_references.iter().map(IriBuf::as_str).collect::<Vec<_>>(),
            vec!["https://example.org/other.jsonld"]
        );
        assert!(table.terms.contains_key("speed"), "the sibling inline declarations are still parsed");
    }

    /// A reference the IRI parser rejects names no document to resolve, so it
    /// demotes the fastpath without joining the walk's worklist.
    #[test]
    fn a_nested_reference_that_is_not_an_iri_is_not_recorded() {
        let table = parse_user_context(&json!({"@context": ["not an iri"]}));
        assert!(table.has_unsupported_feature);
        assert!(table.nested_references.is_empty(), "{:?}", table.nested_references);
    }

    /// JSON-LD 1.1 gives a term definition a string or a map and never an
    /// array. The fastpath must not expand against one, and a scoped `@context`
    /// hidden inside it must still be seen.
    #[test]
    fn an_array_term_declaration_demotes_the_fastpath_and_is_still_searched() {
        let table = parse_user_context(&json!({"speed": [{"@id": "https://example.org/speed", "@context": {}}]}));
        assert!(table.has_unsupported_feature, "an array term definition is not a shape the fastpath models");
        pretty_assertions::assert_eq!(table.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("speed"));
    }

    /// The first scoped term found is the one reported; a second one does not
    /// replace it.
    #[test]
    fn the_first_scoped_term_found_is_the_one_reported() {
        let table = parse_user_context(&json!({
            "alpha": {"@id": "https://example.org/alpha", "@context": {}},
            "beta": {"@id": "https://example.org/beta", "@context": {}}
        }));
        pretty_assertions::assert_eq!(table.scoped_context_term.as_ref().map(ContextTerm::as_str), Some("alpha"));
    }

    /// A JSON-LD keyword the fastpath does not model demotes it, and `@vocab`
    /// is deliberately ignored because clause 5.5.7 gives the core one absolute
    /// precedence.
    #[test]
    fn an_unmodelled_keyword_demotes_the_fastpath_and_a_user_vocab_is_ignored() {
        assert!(parse_user_context(&json!({"@language": "en"})).has_unsupported_feature);
        assert!(!parse_user_context(&json!({"@vocab": "https://example.org/"})).has_unsupported_feature);
    }
}
