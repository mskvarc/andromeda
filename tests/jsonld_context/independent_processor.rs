//! Reading a response the way a plain JSON-LD processor reads it.
//!
//! Clause 4.4 asks that the `@context` a response names leave the core
//! `@context` in force last "so that [...] upstream JSON-LD processors can
//! properly expand as NGSI-LD". The proof is to hand the rendered body, with
//! nothing but the `@context` it names, to a plain JSON-LD 1.1 processor and
//! compare the IRIs it expands to with the IRIs the broker stored. The processor
//! is the `jsonld` crate family on its own: no Andromeda engine, no NGSI-LD
//! precedence rule, no core `@context` it is not handed. It runs offline,
//! resolving only the v1.9 core `@context` exactly as Andromeda embeds it and
//! the documents the test fetched itself.

use crate::core_contexts::core;
use jsonld::{IriBuf, JsonLdProcessor, RemoteDocument};
use jsonld_syntax::IntoJsonWithContext;
use reqwest::Client;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use url::Url;

/// The v1.9 core `@context` exactly as Andromeda embeds it.
const CORE_V19_DOCUMENT: &str = include_str!("../../crates/andromeda-ngsi-ld-vocabulary/context/ngsi-ld-core-context-v1.9.jsonld");

/// The context the v1.9 core `@context`'s `ngsildproof` term scopes. JSON-LD
/// 1.1 API section 4.2.2 step 21.3 processes a scoped context when its term is
/// defined, so it must resolve; no response under test uses the term, so the
/// processor is handed the same empty document the broker registers for it.
const DATA_INTEGRITY_CONTEXT_URL: &str = "https://w3id.org/security/data-integrity/v2";

/// What the processor read one node as.
#[derive(Debug, PartialEq, Eq)]
pub struct ReadNode {
    /// The node's `@id`.
    pub id: Option<String>,
    /// The IRIs of the node's types.
    pub types: BTreeSet<String>,
    /// The IRIs of the node's properties.
    pub properties: BTreeSet<String>,
}

/// An offline JSON-LD 1.1 processor and the documents it may resolve.
pub struct IndependentProcessor {
    documents: HashMap<IriBuf, RemoteDocument>,
}

impl IndependentProcessor {
    /// A processor that resolves the v1.9 core `@context` and nothing else.
    pub fn new() -> IndependentProcessor {
        let mut processor = IndependentProcessor { documents: HashMap::new() };
        processor.insert(core(), serde_json::from_str(CORE_V19_DOCUMENT).expect("the embedded core context is JSON"));
        processor.insert(DATA_INTEGRITY_CONTEXT_URL, serde_json::json!({ "@context": {} }));
        processor
    }

    fn insert(&mut self, url: &str, document: Value) {
        let iri = IriBuf::new(url.to_owned()).expect("a document URL is an IRI");
        let remote = RemoteDocument::new(Some(iri.clone()), None, jsonld_syntax::Value::from_serde_json(document)); // clone: the map key and the document each hold the URL
        self.documents.insert(iri, remote);
    }

    /// Fetch the context document at `url` over HTTP, as a remote processor
    /// would, and every context document it references in turn (relative
    /// references resolved against the document's URL). The v1.9 core
    /// `@context` is never fetched: the embedded copy stands for it.
    pub async fn fetch(mut self, url: &str) -> IndependentProcessor {
        let client = Client::new();
        let mut pending = vec![url.to_owned()];
        while let Some(next) = pending.pop() {
            let key = IriBuf::new(next.clone()).expect("a document URL is an IRI"); // clone: the pending entry is used below
            if self.documents.contains_key(&key) {
                continue;
            }
            let document: Value = client
                .get(&next)
                .send()
                .await
                .expect("the context document is reachable")
                .json()
                .await
                .expect("a JSON context document");
            let base = Url::parse(&next).expect("an absolute document URL");
            pending.extend(
                context_references(&document["@context"])
                    .into_iter()
                    .map(|reference| base.join(&reference).expect("a resolvable context reference").to_string()),
            );
            self.insert(&next, document);
        }
        self
    }

    /// Expand `body`, a JSON-LD document naming its own `@context`, and return
    /// what every top-level node reads as.
    pub async fn read(&self, body: &Value) -> Vec<ReadNode> {
        let input: RemoteDocument = RemoteDocument::new(
            Some(IriBuf::new("https://example.org/response".to_owned()).expect("an IRI")),
            None,
            jsonld_syntax::Value::from_serde_json(body.clone()), // clone: the caller keeps the rendered body
        );
        let expanded = input
            .expand(&self.documents)
            .await
            .map_err(|error| error.to_string())
            .expect("the processor expands the body under the @context it names");
        let expanded = jsonld_syntax::Value::into_serde_json(expanded.into_iter().collect::<Vec<_>>().into_json_with(&()));
        expanded.as_array().expect("an expanded document is an array").iter().map(read_node).collect()
    }

    /// What the Entity `stored`, read back with no request context and so
    /// compacted with the core `@context` alone (clause 5.5.5), reads as under
    /// that core `@context`.
    pub async fn read_stored(&self, stored: &Value) -> ReadNode {
        let mut document = stored.clone(); // clone: the caller keeps the entity for its messages
        document["@context"] = json!(core());
        let mut read = self.read(&document).await;
        assert_eq!(read.len(), 1, "one stored node: {stored}");
        read.remove(0)
    }
}

/// What one expanded node object reads as.
fn read_node(node: &Value) -> ReadNode {
    let object = node.as_object().expect("an expanded node object");
    ReadNode {
        id: object.get("@id").and_then(Value::as_str).map(str::to_owned),
        types: object
            .get("@type")
            .and_then(Value::as_array)
            .map(|types| types.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
        properties: object.keys().filter(|key| !key.starts_with('@')).cloned().collect(), // clone: the set outlives the expanded document
    }
}

/// The string references a `@context` value names, the v1.9 core left out.
fn context_references(context: &Value) -> Vec<String> {
    let mut references = Vec::new();
    collect_references(context, &mut references);
    references.retain(|reference| reference != core());
    references
}

fn collect_references(context: &Value, references: &mut Vec<String>) {
    match context {
        Value::String(reference) => references.push(reference.clone()), // clone: the reference outlives the borrowed document
        Value::Array(entries) => entries.iter().for_each(|entry| collect_references(entry, references)),
        Value::Object(definition) => {
            if let Some(imported) = definition.get("@import") {
                collect_references(imported, references);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
