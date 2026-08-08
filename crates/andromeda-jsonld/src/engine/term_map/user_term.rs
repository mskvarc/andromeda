//! Parsing and caching of request-supplied JSON-LD term declarations.

use crate::{
    engine::{
        core_term_semantics::{coercion_of, container_of},
        term_map::coercion::{TermCoercion, TermContainer},
    },
    error::JsonLdError,
    loader::http::HttpContextLoader,
};
use dashmap::DashMap;
use iri_rs::IriBuf;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// One resolved JSON-LD term declaration.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTerm {
    /// Expanded term IRI.
    pub(crate) iri: IriBuf,
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
    /// Terms defined by the context.
    pub(crate) terms: HashMap<Box<str>, ResolvedTerm>,
    /// Whether any term carries a scoped context.
    pub(crate) has_scoped_contexts: bool,
    /// Whether the context uses unsupported JSON-LD behavior.
    pub(crate) has_unsupported_feature: bool,
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
        has_scoped_contexts: false,
        has_unsupported_feature: false,
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
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Parse one compact-term declaration into its fast-path representation.
fn parse_term_declaration(term: &str, declaration: &Value, table: &mut UserTermTable) {
    match declaration {
        Value::String(iri) => {
            let Some(iri) = iri_from_context(iri) else {
                table.has_unsupported_feature = true;
                return;
            };
            table.terms.insert(
                Box::<str>::from(term),
                ResolvedTerm {
                    iri,
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
            table.has_scoped_contexts |= has_scoped_context;
            let Some(iri) = definition.get("@id").and_then(Value::as_str) else {
                return;
            };
            let Some(iri) = iri_from_context(iri) else {
                table.has_unsupported_feature = true;
                return;
            };
            table.terms.insert(
                Box::<str>::from(term),
                ResolvedTerm {
                    iri,
                    coercion: declared_coercion(definition.get("@type").and_then(Value::as_str)),
                    container: container_of(definition.get("@container").and_then(Value::as_str)),
                    has_scoped_context,
                    simple: false,
                },
            );
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => {}
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
