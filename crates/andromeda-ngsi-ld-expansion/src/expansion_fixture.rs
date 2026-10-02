//! An offline engine resolving the test contexts, and both expansions of one
//! request side by side.
//!
//! A request is read the way the transport reads it -- numbers canonicalised,
//! the body `@context` split off into the request context, a typed body
//! decoded into the compact model -- and then expanded twice: by the typed
//! walk, and by the JSON-LD processor from the serialization the write path
//! hands it today, read through the processor decoding.

use crate::{
    attribute_fragment::AttributeFragment,
    normalization::Normalization,
    processor_decoding::{decode_attribute_fragment, decode_entity},
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{expansion_context::ExpansionContext, processor::JsonLdEngine},
    error::JsonLdError,
};
use andromeda_ngsi_ld_entity::{document::EntityDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use andromeda_ngsi_ld_number::json_value::{canonicalize_numbers, deserialize_exact};
use andromeda_ngsi_ld_representation::normalization::normalize_attribute_node;
use iri_rs::IriBuf;
use jsonld::{ExpandedDocument, syntax::Value as JsonLdValue};
use rdfx::BlankIdBuf;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{collections::HashMap, fmt::Debug, path::PathBuf, sync::Arc};
use url::Url;

/// The test contexts, by the URL a request names them with and their file.
const CONTEXTS: &[(&str, &str)] = &[
    ("https://example.org/contexts/terms.jsonld", "terms.jsonld"),
    ("https://example.org/contexts/vocab-and-language.jsonld", "vocab-and-language.jsonld"),
    ("https://example.org/contexts/unsupported.jsonld", "unsupported.jsonld"),
    ("http://example.org/ngsi-ld/latest/commonTerms.jsonld", "commonTerms.jsonld"),
    ("http://example.org/ngsi-ld/latest/vehicle.jsonld", "vehicle.jsonld"),
    ("http://example.org/ngsi-ld/latest/parking.jsonld", "parking.jsonld"),
    ("http://example.org/ngsi-ld/latest/parking-abbreviated.jsonld", "parking-abbreviated.jsonld"),
    ("http://example.org/ngsi-ld/latest/parking-c7.jsonld", "parking-c7.jsonld"),
    ("http://example.org/ngsi-ld/latest/multiAttribute.jsonld", "multiAttribute.jsonld"),
    ("http://example.org/ngsi-ld/latest/tested-at-context.jsonld", "tested-at-context.jsonld"),
    ("http://example.org/ngsi-ld/latest/schema-stand-in.jsonld", "schema-stand-in.jsonld"),
    ("http://example.org/ngsi-ld/latest/vehicle-v1-context.jsonld", "vehicle-v1-context.jsonld"),
    ("http://example.org/ngsi-ld/latest/place-context.jsonld", "place-context.jsonld"),
    ("https://uri.etsi.org/ngsi-ld/primer/store-context.jsonld", "store-context.jsonld"),
];

/// The URL of the test context declaring every coercion the walk models.
pub const TERMS: &str = "https://example.org/contexts/terms.jsonld";

/// The URL of the test context setting `@vocab` and a default `@language`.
pub const VOCAB_AND_LANGUAGE: &str = "https://example.org/contexts/vocab-and-language.jsonld";

/// The URL of the test context declaring what the walk declines.
pub const UNSUPPORTED: &str = "https://example.org/contexts/unsupported.jsonld";

/// An engine resolving the test contexts and nothing else.
pub fn engine() -> JsonLdEngine {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contexts");
    let preloads: HashMap<Url, PathBuf> = CONTEXTS
        .iter()
        .map(|(url, file)| (Url::parse(url).expect("a fixture context URL"), directory.join(file)))
        .collect();
    JsonLdEngine::builder()
        .allow_remote(false)
        .preload_contexts(preloads)
        .build()
        .expect("the test engine builds")
}

/// Why a fixture could not be expanded both ways at all.
#[derive(Debug)]
pub enum FixtureFailure {
    /// The request's context did not resolve offline.
    Context(JsonLdError),
    /// The body does not decode as the typed compact request.
    NotTyped(serde_json::Error),
    /// The processor refused the document.
    Processor(JsonLdError),
}

impl FixtureFailure {
    /// Why the fixture could not be expanded, for a test's report.
    pub fn reason(&self) -> String {
        match self {
            FixtureFailure::Context(error) => format!("context: {error}"),
            FixtureFailure::NotTyped(error) => format!("not typed: {error}"),
            FixtureFailure::Processor(error) => format!("processor: {error}"),
        }
    }
}

/// Both expansions of one request.
pub struct BothWays<T> {
    /// The typed walk's.
    pub typed: Result<TypedExpansion<T>, EntityRefusal>,
    /// The processor's.
    pub processor: Result<T, EntityRefusal>,
}

impl<T: Debug + PartialEq> BothWays<T> {
    /// The document both expansions agree on; fails when the walk declined
    /// or the two differ.
    pub fn agreed(self) -> T {
        match (self.typed, self.processor) {
            (Ok(TypedExpansion::Expanded(typed)), Ok(processor)) => {
                pretty_assertions::assert_eq!(typed, processor, "the typed walk and the processor disagree");
                typed
            }
            (Ok(TypedExpansion::Declined(decline)), _) => panic!("the typed walk declined: {decline}"),
            (typed, processor) => panic!("expected both to expand, got {typed:?} and {processor:?}"),
        }
    }

    /// The refusal both expansions agree on.
    pub fn refused(self) -> String {
        match (self.typed, self.processor) {
            (Err(typed), Err(processor)) => {
                pretty_assertions::assert_eq!(typed.to_string(), processor.to_string());
                typed.to_string()
            }
            (typed, processor) => panic!("expected both to refuse, got {typed:?} and {processor:?}"),
        }
    }
}

/// A request body as the transport reads it: numbers canonicalised and the
/// body `@context` split off into the request context.
pub fn admitted(mut body: Value) -> (Value, ContextSpec) {
    canonicalize_numbers(&mut body).expect("the fixture's numbers are in range");
    let context = body.as_object_mut().and_then(|members| members.remove("@context"));
    let spec = context.map_or_else(ContextSpec::none, ContextSpec::from_body);
    (body, spec)
}

/// The processed context of a spec.
async fn context(engine: &JsonLdEngine, spec: &ContextSpec) -> Result<Arc<ExpansionContext>, FixtureFailure> {
    engine.expansion_context(spec).await.map_err(FixtureFailure::Context)
}

/// The processor's expansion of a compact document.
async fn processor(engine: &JsonLdEngine, context: &ExpansionContext, document: Value) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, FixtureFailure> {
    context
        .expand(engine, &JsonLdValue::from_serde_json(document))
        .await
        .map_err(FixtureFailure::Processor)
}

/// The serialization the write path hands the processor today.
fn serialized<T: Serialize>(request: &T, normalization: Normalization) -> Value {
    let mut json = serde_json::to_value(request).expect("the typed request serializes");
    normalization.apply(&mut json);
    json
}

/// Decode a body as a typed compact request.
fn typed<T: DeserializeOwned>(body: &Value) -> Result<T, FixtureFailure> {
    deserialize_exact(body).map_err(FixtureFailure::NotTyped)
}

/// Expand `body` decoded as the typed compact `Entity`.
pub async fn entity(engine: &JsonLdEngine, body: Value, normalization: Normalization) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let entity: Entity = typed(&body)?;
    let context = context(engine, &spec).await?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).entity(&entity, normalization),
        processor: decode_entity(processor(engine, &context, serialized(&entity, normalization)).await?),
    })
}

/// Expand `body` decoded as the typed compact `EntityPatch`.
pub async fn patch(engine: &JsonLdEngine, body: Value, normalization: Normalization) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let patch: EntityPatch = typed(&body)?;
    let context = context(engine, &spec).await?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).entity_patch(&patch, normalization),
        processor: decode_entity(processor(engine, &context, serialized(&patch, normalization)).await?),
    })
}

/// Expand `body` held as JSON (append, update).
pub async fn fragment(engine: &JsonLdEngine, body: Value, normalization: Normalization) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    let (mut body, spec) = admitted(body);
    normalization.apply(&mut body);
    let context = context(engine, &spec).await?;
    let typed = TypedExpander::new(&context).normalized_fragment(&body);
    Ok(BothWays {
        typed,
        processor: decode_entity(processor(engine, &context, body).await?),
    })
}

/// The processor's expansion of `body` held as JSON, read by the processor
/// decoding and by the canonical decoder of the entity crate from the
/// expanded text.
pub async fn decoded_both_ways(
    engine: &JsonLdEngine,
    body: Value,
    normalization: Normalization,
) -> Result<(Result<EntityDocument, EntityRefusal>, Result<EntityDocument, EntityRefusal>), FixtureFailure> {
    let (mut body, spec) = admitted(body);
    normalization.apply(&mut body);
    let context = context(engine, &spec).await?;
    let expanded = processor(engine, &context, body).await?;
    let text = serde_json::to_vec(&expanded.clone().into_serde_json()).expect("the expanded document serializes"); // clone: the document is read twice
    Ok((decode_entity(expanded), EntityDocument::from_slice(&text)))
}

/// Expand `body` as the attribute fragment of clauses 5.6.4 and 5.6.19,
/// written for the attribute `name`.
pub async fn attribute(engine: &JsonLdEngine, name: &str, body: Value) -> Result<BothWays<AttributeFragment>, FixtureFailure> {
    let (mut body, spec) = admitted(body);
    normalize_attribute_node(name, &mut body);
    let name: AttributeName = name.parse().expect("an attribute name");
    let context = context(engine, &spec).await?;
    let typed = TypedExpander::new(&context).normalized_attribute(&name, &body);
    let document = Value::Object(Map::from_iter([(name.to_string(), body)]));
    Ok(BothWays {
        typed,
        processor: decode_attribute_fragment(&name, processor(engine, &context, document).await?),
    })
}
