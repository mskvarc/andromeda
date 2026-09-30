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
#[derive(Debug, Default)]
pub(crate) struct UserTermTableCache {
    /// Parsed term tables indexed by their context IRI.
    tables: DashMap<IriBuf, Arc<UserTermTable>>,
}

impl UserTermTableCache {
    /// Resolve and parse a context URL on its first use.
    pub(crate) async fn get_or_load(&self, loader: &HttpContextLoader, url: &IriBuf) -> Result<Arc<UserTermTable>, JsonLdError> {
        if let Some(cached) = self.tables.get(url) {
            return Ok(Arc::clone(cached.value()));
        }
        let value = loader.load_value(url).await?;
        let serde_value = jsonld_syntax::Value::into_serde_json((*value).clone()); // clone: the loader retains its cached document
        let table = Arc::new(parse_user_context(&serde_value));
        self.tables.insert(url.clone(), Arc::clone(&table));
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
    use crate::{context_term::ContextTerm, engine::term_map::user_term::parse_user_context};
    use iri_rs::IriBuf;
    use serde_json::json;

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
