//! Fixture documents for unit tests: JSON written with `json!`, admitted as
//! the literals the crate holds, and the processor's expansion of a document
//! under a context.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    context_value::{EntriesShape, context_entries, written_entries},
    engine::{
        context_document::{ContextDocument, RelativeReferences},
        processor::JsonLdEngine,
    },
    error::JsonLdError,
};
use andromeda_json_literal::JsonLiteral;
use iri_rs::IriBuf;
use jsonld_syntax::IntoJsonWithContext;
use jstrict::Print;
use rdfx::vocabulary::no_vocabulary;
use serde_json::Value;
use std::sync::Arc;

/// `value` admitted as a literal.
pub(crate) fn literal(value: &Value) -> JsonLiteral {
    JsonLiteral::admit(&value.to_string()).expect("an admissible fixture")
}

/// `literal` read back as JSON.
pub(crate) fn as_value(literal: &JsonLiteral) -> Value {
    serde_json::from_str(literal.text()).expect("a literal is JSON")
}

/// The processor's expansion of `document` under `spec`, the document's own
/// `@context` processed before the one the spec names, as JSON.
pub(crate) async fn expanded_document(engine: &JsonLdEngine, document: &Value, spec: &ContextSpec) -> Result<Value, JsonLdError> {
    let mut document = document.clone(); // clone: the fixture stays the caller's
    let own = document.as_object_mut().and_then(|members| members.remove("@context"));
    let spec = match (own, spec.core_policy()) {
        (Some(own), CoreContextPolicy::Append) => {
            let mut entries = spec.request_context().entries();
            let own = literal(&own);
            let mut combined = context_entries(own.piece());
            combined.append(&mut entries);
            ContextSpec::from_body(JsonLiteral::admit(&written_entries(&combined, EntriesShape::Array)).expect("an admissible context"))
        }
        (Some(_) | None, CoreContextPolicy::Only) | (None, CoreContextPolicy::Append) => spec.clone(), // clone: the caller keeps the spec
    };
    let context = engine.expansion_context(&spec).await?;
    let read = context.expand_text(engine, document.to_string().as_bytes()).await?;
    Ok(serde_json::from_str(&read.into_json_with(no_vocabulary()).compact_print().to_string()).expect("an expanded document is JSON"))
}

/// The document the processor reads, as JSON.
pub(crate) fn document_value(document: &ContextDocument) -> Value {
    serde_json::from_str(&document.json().compact_print().to_string()).expect("a document is JSON")
}

/// `document`, written as JSON, prepared as the loader holds a document of no
/// particular URL.
pub(crate) fn context_document(document: &Value) -> Arc<ContextDocument> {
    let url = IriBuf::new("https://example.com/fixture.jsonld".to_owned()).expect("a valid URL");
    Arc::new(
        ContextDocument::prepared(&url, &literal(document), RelativeReferences::AsWritten)
            .expect("the fixture prepares")
            .document,
    )
}
