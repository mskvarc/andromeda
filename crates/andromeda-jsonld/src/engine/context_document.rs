//! A context document as the JSON-LD processor reads it.
//!
//! The loader holds every document it resolves in the form the processor
//! reads, prepared once when the document enters the broker:
//!
//! - Its relative `@context` and `@import` references are resolved against the
//!   URL it was finally read from. JSON-LD 1.1 API section 4.1.2 step 5.2.6
//!   processes a remote context with "the documentUrl of context document for
//!   base URL", the documentUrl being "the final URL of the loaded document"
//!   after redirects and a followed alternate. The processor Andromeda runs on
//!   passes the URL it asked for instead, so a context reached through a
//!   redirect would resolve its relative references against the wrong
//!   directory; an absolute reference resolves to itself whatever base the
//!   processor passes. Only the members context processing resolves against
//!   the base URL are touched: a string in a `@context` slot (steps 5.2.1 and,
//!   through a scoped context, Create Term Definition step 21) and an `@import`
//!   value (step 5.6.3). `@vocab` resolves against the active context's base
//!   IRI, not the context's URL, and is left as written.
//! - Every embedded core context reference is removed, in every `@context`
//!   slot the document holds, so the authoritative core the broker appends
//!   last stays the single occurrence ([`crate::core_context_precedence`]).
//!   The core context document itself is left untouched: that one *is* the
//!   definition, not a redundant import of it.
//!
//! What the document as written leaves in force last is read before either
//! step, and kept beside it as its [`ContextTail`].

use crate::{
    context_graph::reference_iri,
    context_tail::ContextTail,
    core_context::is_ngsi_ld_core_context_url,
    core_context_precedence::{defines_nothing_but_an_import, is_core_context_json_string},
    loader::remote_document::ld_json_media_type,
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_vocabulary::keyword;
use iri_rs::{IriBuf, IriRef};
use jsonld::RemoteDocument;
use jstrict::{Object, Value};

/// A context document, prepared for the JSON-LD processor.
#[derive(Debug)]
pub struct ContextDocument {
    /// The document in the processor's own JSON form.
    json: Value,
}

/// A prepared context document beside what the document as written leaves in
/// force last.
#[derive(Debug)]
pub(crate) struct PreparedContext {
    /// The document as the processor reads it.
    pub(crate) document: ContextDocument,
    /// What the document as written leaves in force last (clause 4.4).
    pub(crate) tail: ContextTail,
}

/// Where a document's relative references resolve.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RelativeReferences<'a> {
    /// Against the URL the document was finally read from, which they are
    /// rewritten to before the processor reads the document.
    ResolvedAgainst(&'a IriBuf),
    /// As written: the processor resolves them against the URL it asked for,
    /// which is the document's own.
    AsWritten,
}

impl ContextDocument {
    /// Prepare `written`, the document resolved under `url`, for the
    /// processor.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::ProcessorText`] when the processor's reader
    /// refuses the admitted text.
    pub(crate) fn prepared(url: &IriBuf, written: &JsonLiteral, relative: RelativeReferences<'_>) -> Result<PreparedContext, JsonLiteralError> {
        let tail = ContextTail::of_document(written.piece());
        let mut json = written.to_processor_json()?;
        if let RelativeReferences::ResolvedAgainst(document_url) = relative {
            resolve_relative_context_references(&mut json, document_url);
        }
        if !is_ngsi_ld_core_context_url(url.as_ref()) {
            remove_embedded_core_references(&mut json);
        }
        Ok(PreparedContext {
            document: ContextDocument { json },
            tail,
        })
    }

    /// The remote document the processor reads, based on `base`.
    pub(crate) fn remote_document(&self, base: IriBuf) -> RemoteDocument<IriBuf> {
        // clone: the processor consumes its copy while the loader keeps the document
        RemoteDocument::new(Some(base), Some(ld_json_media_type()), self.json.clone())
    }

    /// The references the document's `@context` names, each resolved against
    /// `base`, the URL the processor resolves them against, when the document
    /// was loaded from one: a string entry of the slot (JSON-LD 1.1 API
    /// section 4.1.2 step 5.2) and an `@import` value (step 5.6).
    pub(crate) fn context_references(&self, base: Option<&IriBuf>) -> Vec<IriBuf> {
        let mut references = Vec::new();
        if let Value::Object(members) = &self.json
            && let Ok(Some(slot)) = members.get_unique(keyword::CONTEXT)
        {
            collect_slot_references(slot, base, &mut references);
        }
        references
    }

    /// The document in the processor's own JSON form.
    pub(crate) const fn json(&self) -> &Value {
        &self.json
    }
}

/// Collect the references of one `@context` slot.
fn collect_slot_references(slot: &Value, base: Option<&IriBuf>, references: &mut Vec<IriBuf>) {
    match slot {
        Value::String(reference) => references.extend(reference_iri(reference.as_str(), base)),
        Value::Array(entries) => entries.iter().for_each(|entry| collect_slot_references(entry, base, references)),
        Value::Object(definition) => {
            if let Ok(Some(Value::String(import))) = definition.get_unique(keyword::IMPORT) {
                references.extend(reference_iri(import.as_str(), base));
            }
        }
        Value::Null | Value::Boolean(_) | Value::Number(_) => {}
    }
}

/// Resolve every relative context reference in `document` against
/// `document_url`, the URL it was finally read from.
///
/// A reference that is not a valid IRI reference, or does not resolve, is left
/// as written: the processor then reports it exactly as it would have.
fn resolve_relative_context_references(document: &mut Value, document_url: &IriBuf) {
    if let Value::Object(members) = document
        && let Ok(Some(slot)) = members.get_unique_mut(keyword::CONTEXT)
    {
        resolve_slot(slot, document_url);
    }
}

/// Resolve the references of one `@context` slot: a reference, a definition, or
/// an array of both.
fn resolve_slot(slot: &mut Value, document_url: &IriBuf) {
    match slot {
        Value::String(reference) => {
            if let Some(resolved) = resolved_reference(reference.as_str(), document_url) {
                *reference = resolved.into();
            }
        }
        Value::Object(definition) => resolve_definition(definition, document_url),
        Value::Array(entries) => entries.iter_mut().for_each(|entry| resolve_slot(entry, document_url)),
        Value::Null | Value::Boolean(_) | Value::Number(_) => {}
    }
}

/// Resolve a context definition's `@import` and the scoped contexts of its term
/// definitions.
fn resolve_definition(definition: &mut Object, document_url: &IriBuf) {
    for (key, value) in definition.iter_mut() {
        match value {
            Value::String(import) if key.as_str() == keyword::IMPORT => {
                if let Some(resolved) = resolved_reference(import.as_str(), document_url) {
                    *import = resolved.into();
                }
            }
            Value::Object(term_definition) => {
                if let Ok(Some(scoped)) = term_definition.get_unique_mut(keyword::CONTEXT) {
                    resolve_slot(scoped, document_url);
                }
            }
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
        }
    }
}

/// The resolution of a relative `reference` against `document_url`, or `None`
/// for an absolute reference, one that is no IRI reference, or one that does
/// not resolve.
fn resolved_reference(reference: &str, document_url: &IriBuf) -> Option<String> {
    let parsed = IriRef::parse(reference).ok()?;
    if parsed.is_absolute() {
        return None;
    }
    let mut resolved = String::with_capacity(document_url.as_str().len() + reference.len());
    parsed.resolve_into(document_url, &mut resolved).ok()?;
    Some(resolved)
}

/// Walk a document and drop core context references from every `@context`.
fn remove_embedded_core_references(value: &mut Value) {
    match value {
        Value::Object(members) => {
            if let Ok(Some(context)) = members.get_unique_mut(keyword::CONTEXT) {
                strip_core_references_from_context_slot(context);
            }
            for (_, inner) in members.iter_mut() {
                remove_embedded_core_references(inner);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(remove_embedded_core_references),
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Drop core context references from one `@context` slot.
///
/// Every core reference is dropped, so a slot that names the core twice is left
/// with none: the broker supplies the one authoritative copy, last.
fn strip_core_references_from_context_slot(context: &mut Value) {
    match context {
        Value::String(url) if is_core_context_json_string(url.as_str()) => {
            *context = Value::Array(Vec::new().into());
        }
        Value::Array(items) => {
            items.retain(|item| !item.as_str().is_some_and(is_core_context_json_string));
            for item in items.iter_mut() {
                if let Value::Object(definition) = item {
                    drop_core_import(definition);
                }
                remove_embedded_core_references(item);
            }
        }
        Value::Object(definition) => {
            drop_core_import(definition);
            remove_embedded_core_references(context);
        }
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Drop an `@import` of a core context from one context definition, emptying a
/// definition that defined nothing else ([`defines_nothing_but_an_import`]).
fn drop_core_import(definition: &mut Object) {
    let imports_core = matches!(definition.get_unique(keyword::IMPORT), Ok(Some(Value::String(import))) if is_core_context_json_string(import.as_str()));
    if !imports_core {
        return;
    }
    if defines_nothing_but_an_import(definition.iter().map(|entry| entry.key.as_str())) {
        *definition = Object::new();
    } else {
        // The admitted document names every member once, so this removes the
        // one import.
        definition.remove(keyword::IMPORT).for_each(drop);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_tail::ContextTail,
        core_context::{NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
        engine::context_document::{ContextDocument, RelativeReferences},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::{IriBuf, iri};
    use jstrict::Print;
    use serde_json::{Value, json};

    fn url(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("a valid URL")
    }

    /// Prepare `document` as if it were loaded from `url`, its references as
    /// written, and read the prepared copy back as JSON.
    fn prepare(at: &str, document: &Value) -> (Value, ContextTail) {
        prepare_with(at, document, RelativeReferences::AsWritten)
    }

    fn prepare_with(at: &str, document: &Value, relative: RelativeReferences<'_>) -> (Value, ContextTail) {
        let written = JsonLiteral::admit(&document.to_string()).expect("an admissible document");
        let prepared = ContextDocument::prepared(&url(at), &written, relative).expect("the document prepares");
        let read = serde_json::from_str(&prepared.document.json().compact_print().to_string()).expect("the prepared document is JSON");
        (read, prepared.tail)
    }

    #[test]
    fn a_user_context_importing_the_core_loses_only_that_redundant_import() {
        let (normalized, tail) = prepare(
            "https://example.com/compound.jsonld",
            &json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL.as_str(), { "location": "https://example.com/location" }] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": [{ "location": "https://example.com/location" }] }));
        pretty_assertions::assert_eq!(tail, ContextTail::LocalDefinitions);
    }

    #[test]
    fn a_context_slot_naming_the_core_twice_is_left_with_none() {
        let (normalized, _) = prepare(
            "https://example.com/twice.jsonld",
            &json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL.as_str(), NGSI_LD_CORE_CONTEXT_URL.as_str(), { "speed": "https://example.com/speed" }] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": [{ "speed": "https://example.com/speed" }] }));
    }

    /// Clause 4.4 and Annex C.8: what the document leaves in force last is read
    /// from the document as written, before its core reference is dropped for
    /// processing.
    #[test]
    fn a_document_ending_with_the_core_keeps_that_fact_although_the_reference_is_dropped() {
        let (normalized, tail) = prepare(
            "https://example.com/compound.jsonld",
            &json!({ "@context": ["https://example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] }),
        );

        pretty_assertions::assert_eq!(normalized, json!({ "@context": ["https://example.com/terms.jsonld"] }));
        pretty_assertions::assert_eq!(tail, ContextTail::CurrentCore);
    }

    /// JSON-LD 1.1 section 4.1.10 and clause 5.5.7: an imported core is an
    /// earlier, protected occurrence exactly as a referenced one is, so the
    /// import is dropped in every release.
    #[test]
    fn an_imported_core_of_any_release_is_dropped() {
        for core in [NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_URL] {
            let (normalized, _) = prepare(
                "https://example.com/importing.jsonld",
                &json!({ "@context": { "@import": core.as_str(), "speed": "https://example.com/speed" } }),
            );

            pretty_assertions::assert_eq!(normalized, json!({ "@context": { "speed": "https://example.com/speed" } }), "{core}");
        }
        let (in_array, _) = prepare(
            "https://example.com/importing.jsonld",
            &json!({ "@context": ["https://example.com/a.jsonld", { "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str() }] }),
        );
        pretty_assertions::assert_eq!(in_array, json!({ "@context": ["https://example.com/a.jsonld", {}] }));
    }

    /// A definition that imported the core and set nothing else but `@version`
    /// or `@protected` defined nothing of its own, so nothing of it remains
    /// once the import is dropped.
    #[test]
    fn a_definition_left_with_only_version_or_protected_after_its_core_import_is_emptied() {
        for remainder in [
            json!({ "@version": 1.1 }),
            json!({ "@protected": true }),
            json!({ "@version": 1.1, "@protected": true }),
        ] {
            let mut definition = remainder.as_object().expect("an object").clone(); // clone: the fixture keeps its remainder for the message
            definition.insert("@import".to_owned(), json!(NGSI_LD_CORE_CONTEXT_V18_URL.as_str()));

            let (normalized, _) = prepare("https://example.com/importing.jsonld", &json!({ "@context": definition }));

            pretty_assertions::assert_eq!(normalized, json!({ "@context": {} }), "{remainder} should leave nothing behind");
        }
        let (with_a_term, _) = prepare(
            "https://example.com/importing.jsonld",
            &json!({ "@context": { "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "@version": 1.1, "speed": "https://example.com/speed" } }),
        );
        pretty_assertions::assert_eq!(with_a_term, json!({ "@context": { "@version": 1.1, "speed": "https://example.com/speed" } }));
    }

    #[test]
    fn an_import_of_a_user_context_is_kept() {
        let document = json!({ "@context": { "@import": "https://example.com/base.jsonld", "speed": "https://example.com/speed" } });
        let (normalized, _) = prepare("https://example.com/importing.jsonld", &document);

        pretty_assertions::assert_eq!(normalized, document);
    }

    #[test]
    fn the_core_context_document_itself_is_left_alone() {
        let core_url = IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"));
        let payload = json!({ "@context": [core_url.as_str(), { "location": "https://example.com/location" }] });

        let (normalized, _) = prepare(core_url.as_str(), &payload);

        pretty_assertions::assert_eq!(normalized, payload);
    }

    /// JSON-LD 1.1 API section 4.1.2 step 5.2.6: a downloaded document's
    /// relative references resolve against the URL it was finally read from,
    /// in the top-level slot, in an `@import` and in a scoped context, while
    /// `@vocab` and absolute references are left as written.
    #[test]
    fn relative_references_resolve_against_the_document_url() {
        let final_url = url("https://origin.example.com/v2/terms.jsonld");
        let (resolved, _) = prepare_with(
            "https://example.com/terms.jsonld",
            &json!({ "@context": [
                "base.jsonld",
                "https://other.example.com/absolute.jsonld",
                { "@import": "../shared.jsonld", "@vocab": "relative-vocab#", "scoped": { "@id": "https://example.com/scoped", "@context": "scoped.jsonld" } }
            ] }),
            RelativeReferences::ResolvedAgainst(&final_url),
        );

        pretty_assertions::assert_eq!(
            resolved,
            json!({ "@context": [
                "https://origin.example.com/v2/base.jsonld",
                "https://other.example.com/absolute.jsonld",
                { "@import": "https://origin.example.com/shared.jsonld", "@vocab": "relative-vocab#", "scoped": { "@id": "https://example.com/scoped", "@context": "https://origin.example.com/v2/scoped.jsonld" } }
            ] })
        );
    }

    /// The reference walk follows the string entries and the `@import` of the
    /// document's own slot, resolved against the URL it was loaded from.
    #[test]
    fn the_references_of_the_slot_are_resolved_against_the_document_url() {
        let written = JsonLiteral::admit(&json!({ "@context": ["next.jsonld", { "@import": "https://example.com/imported.jsonld" }] }).to_string())
            .expect("an admissible document");
        let prepared = ContextDocument::prepared(&url("https://example.com/a/doc.jsonld"), &written, RelativeReferences::AsWritten).expect("prepares");

        let references = prepared.document.context_references(Some(&url("https://example.com/a/doc.jsonld")));

        pretty_assertions::assert_eq!(
            references,
            vec![url("https://example.com/a/next.jsonld"), url("https://example.com/imported.jsonld")]
        );
        pretty_assertions::assert_eq!(prepared.document.context_references(None), vec![url("https://example.com/imported.jsonld")]);
    }
}
