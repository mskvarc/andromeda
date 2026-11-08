//! An offline engine resolving the test contexts, and both expansions of one
//! request side by side.
//!
//! A request is read the way the transport reads it (numbers canonicalised,
//! the body `@context` split off into the request context, a typed body
//! decoded into the compact model) and then expanded twice: by the typed
//! walk, and by the JSON-LD processor from the serialization the write path
//! hands it, read through the processor decoding.

use crate::{
    attribute_fragment::AttributeFragment,
    error::ExpansionError,
    normalization::Normalization,
    processor_decoding::{decode_attribute_fragment, decode_entity, decode_temporal},
    processor_request::{attribute_text, entity_text, patch_text},
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
    written_member::WrittenMember,
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{expansion_context::ExpansionContext, processor::JsonLdEngine},
    error::JsonLdError,
};
use andromeda_ngsi_ld_entity::{document::EntityDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
    temporal::{temporal_document::TemporalDocument, temporal_entity::TemporalEntity},
};
use andromeda_ngsi_ld_number::json_value::text_admission::admit_json_text;
use andromeda_ngsi_ld_representation::compact_node::produced_node::ProducedNode;
use iri_rs::IriBuf;
use jsonld::ExpandedDocument;
use rdfx::BlankIdBuf;
use serde::de::{DeserializeOwned, Error as _};
use serde_json::{Value, value::RawValue};
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
pub(crate) const TERMS: &str = "https://example.org/contexts/terms.jsonld";

/// The URL of the test context setting `@vocab` and a default `@language`.
pub(crate) const VOCAB_AND_LANGUAGE: &str = "https://example.org/contexts/vocab-and-language.jsonld";

/// The URL of the test context declaring what the walk declines.
pub(crate) const UNSUPPORTED: &str = "https://example.org/contexts/unsupported.jsonld";

/// An engine resolving the test contexts and nothing else.
pub(crate) fn engine() -> JsonLdEngine {
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
pub(crate) enum FixtureFailure {
    /// The request's context did not resolve offline.
    Context(JsonLdError),
    /// The body does not decode as the typed compact request.
    NotTyped(serde_json::Error),
    /// The processor refused the document.
    Processor(JsonLdError),
}

impl FixtureFailure {
    /// Why the fixture could not be expanded, for a test's report.
    pub(crate) fn reason(&self) -> String {
        match self {
            FixtureFailure::Context(error) => format!("context: {error}"),
            FixtureFailure::NotTyped(error) => format!("not typed: {error}"),
            FixtureFailure::Processor(error) => format!("processor: {error}"),
        }
    }
}

/// Both expansions of one request.
pub(crate) struct BothWays<T> {
    /// The typed walk's.
    pub typed: Result<TypedExpansion<T>, ExpansionError>,
    /// The processor's.
    pub processor: Result<T, EntityRefusal>,
}

impl<T: Debug + PartialEq> BothWays<T> {
    /// The document both expansions agree on; fails when the walk declined
    /// or the two differ.
    pub(crate) fn agreed(self) -> T {
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
    pub(crate) fn refused(self) -> String {
        match (self.typed, self.processor) {
            (Err(typed), Err(processor)) => {
                pretty_assertions::assert_eq!(typed.to_string(), processor.to_string());
                typed.to_string()
            }
            (typed, processor) => panic!("expected both to refuse, got {typed:?} and {processor:?}"),
        }
    }

    /// The refusal of the typed expander, which checks the written names
    /// before either expansion path runs.
    pub(crate) fn refused_before_expansion(self) -> String {
        match self.typed {
            Err(refusal) => refusal.to_string(),
            Ok(typed) => panic!("expected the written names to be refused, got {typed:?}"),
        }
    }
}

/// A request body as the transport reads it: numbers canonicalised and the
/// body `@context` split off into the request context.
pub(crate) fn admitted(mut body: Value) -> (Value, ContextSpec) {
    let text = serde_json::to_vec(&body).expect("a fixture body serialises");
    admit_json_text(&text).expect("the fixture's numbers are in range");
    let context = body.as_object_mut().and_then(|members| members.remove("@context"));
    let spec = context.map_or_else(ContextSpec::none, |context| {
        ContextSpec::from_body(JsonLiteral::admit(&context.to_string()).expect("the fixture's context is admissible"))
    });
    (body, spec)
}

/// The processed context of a spec.
async fn context(engine: &JsonLdEngine, spec: &ContextSpec) -> Result<Arc<ExpansionContext>, FixtureFailure> {
    engine.expansion_context(spec).await.map_err(FixtureFailure::Context)
}

/// The processor's expansion of a compact document, handed to it as `text`.
async fn processor(engine: &JsonLdEngine, context: &ExpansionContext, text: &[u8]) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, FixtureFailure> {
    context.expand_text(engine, text).await.map_err(FixtureFailure::Processor)
}

/// Decode a body as a typed compact request.
fn typed<T: DeserializeOwned>(body: &Value) -> Result<T, FixtureFailure> {
    serde_json::from_str(&body.to_string()).map_err(FixtureFailure::NotTyped)
}

/// The text the write path hands the processor for a typed request.
fn handed(text: Result<Vec<u8>, ExpansionError>) -> Result<Vec<u8>, FixtureFailure> {
    text.map_err(|error| FixtureFailure::NotTyped(serde_json::Error::custom(error)))
}

/// Expand `body` decoded as the typed compact `Entity`.
pub(crate) async fn entity(engine: &JsonLdEngine, body: Value, normalization: Normalization) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let entity: Entity = typed(&body)?;
    let context = context(engine, &spec).await?;
    let text = handed(entity_text(&entity, normalization))?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).entity(&entity, normalization),
        processor: decode_entity(processor(engine, &context, &text).await?),
    })
}

/// Expand `body` decoded as the typed compact `EntityPatch` of the write
/// `normalization` names: an update fragment, or a merge fragment.
pub(crate) async fn patch(engine: &JsonLdEngine, body: Value, normalization: Normalization) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    match normalization {
        Normalization::Entity => patch_of::<AttributeValue>(engine, body, normalization).await,
        Normalization::MemberDeletions => patch_of::<MergeMember>(engine, body, normalization).await,
    }
}

/// Expand `body` decoded as the typed compact `EntityPatch` of members `M`.
async fn patch_of<M: WrittenMember + MemberReading>(
    engine: &JsonLdEngine,
    body: Value,
    normalization: Normalization,
) -> Result<BothWays<EntityDocument>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let patch: EntityPatch<M> = typed(&body)?;
    let context = context(engine, &spec).await?;
    let text = handed(patch_text(&patch, normalization))?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).entity_patch(&patch, normalization),
        processor: decode_entity(processor(engine, &context, &text).await?),
    })
}

/// The processor's expansion of `body` decoded as the typed compact
/// `EntityPatch`, read by the processor decoding and by the canonical decoder
/// of the entity crate from the expanded text.
pub(crate) async fn decoded_both_ways(
    engine: &JsonLdEngine,
    body: Value,
    normalization: Normalization,
) -> Result<(Result<EntityDocument, EntityRefusal>, Result<EntityDocument, EntityRefusal>), FixtureFailure> {
    let (body, spec) = admitted(body);
    let patch: EntityPatch = typed(&body)?;
    let context = context(engine, &spec).await?;
    let text = handed(patch_text(&patch, normalization))?;
    let expanded = processor(engine, &context, &text).await?;
    let text = serde_json::to_vec(&expanded.clone().into_serde_json()).expect("the expanded document serializes"); // clone: the expanded document is still decoded after its text is taken
    Ok((decode_entity(expanded), EntityDocument::from_slice(&text)))
}

/// Expand `body` as the attribute fragment of clauses 5.6.4 and 5.6.19,
/// written for the attribute `name`.
pub(crate) async fn attribute(engine: &JsonLdEngine, name: &str, body: Value) -> Result<BothWays<AttributeFragment>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let written = RawValue::from_string(body.to_string()).map_err(FixtureFailure::NotTyped)?;
    let member = MergeMember::from_fragment_node(name, ProducedNode::new(JsonPiece::new(&written)))
        .map_err(|error| FixtureFailure::NotTyped(serde_json::Error::custom(error)))?;
    let name: AttributeName = name.parse().expect("an attribute name");
    let context = context(engine, &spec).await?;
    let text = handed(attribute_text(&name, &member))?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).attribute(&name, &member),
        processor: decode_attribute_fragment(&name, processor(engine, &context, &text).await?),
    })
}

/// Expand `body` decoded as the Temporal Evolution of an entity, its clause 4.8
/// system times travelling beside the instances.
pub(crate) async fn temporal(engine: &JsonLdEngine, body: Value) -> Result<BothWays<TemporalDocument>, FixtureFailure> {
    let (body, spec) = admitted(body);
    let evolution: TemporalEntity = typed(&body)?;
    let (entity, system_times) = evolution
        .into_entity()
        .map_err(|error| FixtureFailure::NotTyped(serde_json::Error::custom(error)))?;
    let context = context(engine, &spec).await?;
    let text = handed(entity_text(&entity, Normalization::Entity))?;
    Ok(BothWays {
        typed: TypedExpander::new(&context).temporal(&entity, system_times),
        processor: decode_temporal(processor(engine, &context, &text).await?, system_times),
    })
}
