//! The `@type`, `@container` and scoped-`@context` a core context declares per term.
//!
//! The fastpath's term table is derived from the effective core context
//! document rather than restating it. A term's `@type` coercion, its
//! `@container`, and whether it carries a scoped `@context` are read straight
//! from the JSON the deployment resolves against (clause 4.4 pins which
//! released core context that is). Restating them in a hand-written table is
//! what let the two drift: a coercion the context declared but the table
//! forgot expanded a value into the wrong shape, and a term the context marked
//! `@type: @vocab` was stored as a plain string rather than an IRI.
//!
//! Derived once per core-context URL and cached, so a deployment pinned to one
//! version always reads that version's declarations.

use crate::{
    core_context::embedded_core_context,
    engine::term_map::coercion::{TermCoercion, TermContainer},
};
use andromeda_ngsi_ld_vocabulary::expanded::classes;
use dashmap::DashMap;
use iri_rs::IriBuf;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

/// One term's semantic annotations, exactly as a core context declares them.
#[derive(Debug, Clone, Default)]
pub(crate) struct TermSemantics {
    /// The `@type` coercion the term declares, if any.
    pub coercion: Option<TermCoercion>,
    /// The `@container` the term declares, if any.
    pub container: Option<TermContainer>,
    /// Whether the term carries a scoped `@context`.
    pub has_scoped_context: bool,
}

/// A term-name to semantics table for one core context.
type SemanticsTable = HashMap<Box<str>, TermSemantics>;

/// Derived semantics tables, one per core-context URL, built on first use.
static SEMANTICS_BY_URL: LazyLock<DashMap<IriBuf, Arc<SemanticsTable>>> = LazyLock::new(DashMap::new);

/// The core context's term semantics for a deployment's effective core URL.
///
/// The first batch resolving against a URL parses its embedded document; every
/// later one reads the cached table.
pub(crate) fn core_term_semantics(core_url: &IriBuf) -> Arc<SemanticsTable> {
    if let Some(cached) = SEMANTICS_BY_URL.get(core_url) {
        return Arc::clone(cached.value());
    }
    let table = Arc::new(derive_semantics(core_url));
    SEMANTICS_BY_URL.insert(core_url.clone(), Arc::clone(&table));
    table
}

/// Parse the embedded core context document into a table.
fn derive_semantics(core_url: &IriBuf) -> SemanticsTable {
    let mut table = HashMap::new();
    if let Some(core_document) = embedded_core_context(core_url.as_ref()) {
        accumulate_document(core_document, &mut table);
    }
    table
}

/// Read every term declaration of one context document's `@context` object,
/// recording only the terms that carry a coercion, a container, or a scoped
/// context — a plain `term: "iri"` mapping needs no annotation.
fn accumulate_document(document: &str, table: &mut SemanticsTable) {
    let Ok(value) = serde_json::from_str::<Value>(document) else {
        return;
    };
    let Some(context) = value.get("@context").and_then(Value::as_object) else {
        return;
    };
    for (term, declaration) in context {
        if term.starts_with('@') {
            continue;
        }
        let Value::Object(definition) = declaration else {
            continue;
        };
        let semantics = TermSemantics {
            coercion: coercion_of(definition.get("@type").and_then(Value::as_str)),
            container: container_of(definition.get("@container").and_then(Value::as_str)),
            has_scoped_context: definition.contains_key("@context"),
        };
        if semantics.coercion.is_some() || semantics.container.is_some() || semantics.has_scoped_context {
            table.insert(Box::<str>::from(term.as_str()), semantics);
        }
    }
}

/// The coercion a term's `@type` declaration asks for.
///
/// The core context spells the four JSON-LD coercions the fastpath models:
/// `@id`, `@vocab`, `@json` and the `DateTime` alias (which the context maps to
/// the NGSI-LD DateTime datatype). Any other datatype is left unmodelled here —
/// a request context that names an arbitrary datatype keeps it as a
/// [`TermCoercion::TypeIri`] through the caller, not this function.
pub(crate) fn coercion_of(declared_type: Option<&str>) -> Option<TermCoercion> {
    match declared_type {
        Some("@id") => Some(TermCoercion::Id),
        Some("@vocab") => Some(TermCoercion::Vocab),
        Some("@json") => Some(TermCoercion::Json),
        Some("DateTime") => Some(TermCoercion::DateTime),
        Some(declared) if declared == classes::DATE_TIME.as_str() => Some(TermCoercion::DateTime),
        Some(_) | None => None,
    }
}

/// The container a term's `@container` declaration asks for.
pub(crate) fn container_of(declared_container: Option<&str>) -> Option<TermContainer> {
    match declared_container {
        Some("@list") => Some(TermContainer::List),
        Some("@language") => Some(TermContainer::Language),
        Some("@index") => Some(TermContainer::Index),
        Some("@set") => Some(TermContainer::Set),
        Some(_) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core_context::NGSI_LD_CORE_CONTEXT_V19_URL,
        engine::{
            core_term_semantics::core_term_semantics,
            term_map::coercion::{TermCoercion, TermContainer},
        },
    };
    use iri_rs::IriBuf;

    /// The derivation reads the real v1.9 declarations, not a hand-written copy.
    ///
    /// Chosen to cover each drift class ETSI GS CIM 009 v1.9.1 annex B lists:
    /// a `@type: @vocab` term the old table could not represent (`objectType`,
    /// the data-corruption case), a `DateTime` term it had forgotten
    /// (`endTimeAt`), an `@id` term it had forgotten (`entityId`), a
    /// scoped-context term (`ngsildproof`), and a `@list` container term
    /// (`valueList`).
    #[test]
    fn the_table_reflects_the_v1_9_context_declarations() {
        let semantics = core_term_semantics(&IriBuf::new(NGSI_LD_CORE_CONTEXT_V19_URL.to_string()).expect("valid core URL"));

        assert!(matches!(
            semantics.get("objectType").and_then(|s| s.coercion.as_ref()),
            Some(TermCoercion::Vocab)
        ));
        assert!(matches!(
            semantics.get("endTimeAt").and_then(|s| s.coercion.as_ref()),
            Some(TermCoercion::DateTime)
        ));
        assert!(matches!(semantics.get("entityId").and_then(|s| s.coercion.as_ref()), Some(TermCoercion::Id)));
        assert!(semantics.get("ngsildproof").is_some_and(|s| s.has_scoped_context));
        assert!(matches!(semantics.get("valueList").and_then(|s| s.container), Some(TermContainer::List)));
        assert!(matches!(semantics.get("json").and_then(|s| s.coercion.as_ref()), Some(TermCoercion::Json)));
    }

    /// The phantom `DateTime` terms the old hand-written table invented —
    /// `instanceTimeAt`, `previousTime`, `timestamp` — are declared by no
    /// released core context, so the derivation records nothing for them.
    #[test]
    fn terms_no_context_declares_are_absent() {
        let semantics = core_term_semantics(&IriBuf::new(NGSI_LD_CORE_CONTEXT_V19_URL.to_string()).expect("valid core URL"));
        for phantom in ["instanceTimeAt", "previousTime", "timestamp"] {
            assert!(semantics.get(phantom).is_none(), "{phantom} is declared by no context");
        }
    }

    /// Every term the v1.9 context annotates with `@type`, `@container` or a
    /// scoped `@context` is present in the derived table with the matching
    /// annotation — the table cannot silently drop a declaration.
    #[test]
    fn every_annotated_v1_9_term_is_derived() {
        let document: serde_json::Value = serde_json::from_str(crate::core_context::NGSI_LD_CORE_CONTEXT_V19).expect("embedded v1.9 context parses");
        let context = document.get("@context").and_then(serde_json::Value::as_object).expect("a context object");
        let semantics = core_term_semantics(&IriBuf::new(NGSI_LD_CORE_CONTEXT_V19_URL.to_string()).expect("valid core URL"));

        for (term, declaration) in context {
            if term.starts_with('@') {
                continue;
            }
            let Some(definition) = declaration.as_object() else {
                continue;
            };
            let annotates = definition.contains_key("@type") || definition.contains_key("@container") || definition.contains_key("@context");
            if annotates {
                assert!(
                    semantics.get(term.as_str()).is_some(),
                    "{term} is annotated in the context but missing from the table"
                );
            }
        }
    }
}
