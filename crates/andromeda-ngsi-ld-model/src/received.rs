//! Documents received from a peer: a context source's answer, a notification
//! another broker sent (clauses 5.7, 5.8.6, 5.11, 6.3.17).
//!
//! A peer's array is read element by element, each element from the slice of
//! text it is, so an element the model cannot hold is reported and skipped
//! rather than failing the whole document, and no JSON tree is built on the
//! way. An element is refused, never carried unread: clause 4.3.6.8 has a
//! broker "reject data that is invalid", and clause 6.3.17 reports it with a
//! warning.
//!
//! A peer spells an entity under a context too, and may name a core term by a
//! term of it and shape a member by the container of its term (clause 5.5.7,
//! JSON-LD 1.1 API section 6.2.2), so an entity is held as written and read
//! into the model only once the context it was written in was processed:
//! through the aliases of that context and without its containers, as the
//! current state of an entity or as a Temporal Evolution (clauses 5.7.3.4 and
//! 5.7.4.4). The `id` is read on receipt under the clause 4.4 keyword
//! spellings every context reads.

use crate::{
    entity::Entity,
    node_refusal::{NodeReadError, RefusedNode},
    temporal::temporal_body::TemporalBody,
};
use andromeda_json_literal::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_number::json_value::text_admission::{JsonTextRefusal, admit_json_text};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, context_reading::ContextReading};
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use std::convert::Infallible;
use thiserror::Error;

/// One element of a received array, decoded on its own.
#[derive(Debug)]
pub enum ReceivedElement<T> {
    /// The element, read.
    Read(T),
    /// The element, refused.
    Refused(ElementRefusal),
}

/// Why one element of a received array is not read.
#[derive(Debug, Error)]
#[error("element {index}{} is refused: {reason}", .id.as_ref().map_or_default(|id| format!(" ({id})")))]
pub struct ElementRefusal {
    /// Where the element stands in the array, from zero.
    pub index: usize,
    /// The `id` the element names, when it names a readable one.
    pub id: Option<Identifier>,
    /// What is wrong with it.
    #[source]
    pub reason: ElementRefusalReason,
}

/// What is wrong with a refused element.
#[derive(Debug, Error)]
pub enum ElementRefusalReason {
    /// The element is refused as what it was read as.
    #[error(transparent)]
    Refused(RefusedNode),
    /// The element's own `@context` is no admissible JSON value.
    #[error("its @context is not admissible JSON: {0}")]
    Context(#[source] JsonLiteralError),
    /// The element does not decode as the resource it stands for.
    #[error(transparent)]
    Undecodable(serde_json::Error),
    /// The element's text is refused as the text of a request is: an object of
    /// it names one member twice (IETF RFC 8259 section 4), or it holds a
    /// number outside the supported range, a string the broker cannot hold or
    /// a member named like the JSON reader's own tokens. A broker "reject[s] data that is invalid"
    /// (clause 4.3.6.8), and a peer's data is judged as a client's is.
    #[error(transparent)]
    Inadmissible(JsonTextRefusal),
}

/// Why a received document is not read at all.
#[derive(Debug, Error)]
pub enum ReceivedDocumentError {
    /// The text is no JSON.
    #[error("the document is not JSON: {0}")]
    Syntax(#[source] serde_json::Error),
    /// The document is JSON, but neither an array nor an object.
    #[error("the document is neither an array nor an object")]
    NotAnElementOrArray,
    /// The document is an array where a single object is expected.
    #[error("the document is an array where a single entity is expected")]
    ArrayForElement,
}

/// A compact entity as a peer wrote it, with the `@context` it named for
/// itself (JSON-LD 1.1: a node object's own context applies to that node).
#[derive(Debug)]
pub struct ContextualEntity {
    /// The entity's own `@context`, when it named one.
    pub context: Option<JsonLiteral>,
    /// The `id` the entity names as `id` or `@id` (clause 4.4), when it names
    /// a readable one.
    pub id: Option<Identifier>,
    /// Where the entity stands in the array it was received in, from zero.
    index: usize,
    /// The entity as written, its own `@context` among its members.
    written: Box<RawValue>,
}

impl ContextualEntity {
    /// The entity, read as `reading` reads a node written under its context,
    /// its `@context` member not among its members.
    ///
    /// # Errors
    ///
    /// Returns the [`ElementRefusal`] of an entity the model cannot hold,
    /// naming the element and the `id` it names, located in the entity.
    pub fn entity(&self, reading: ContextReading<'_>) -> Result<Entity, Box<ElementRefusal>> {
        Entity::from_node(reading.read(JsonPiece::new(&self.written))).map_err(|error| self.refusal(error))
    }

    /// The Temporal Evolution, the instances of its attributes or the
    /// aggregates over them, read as `reading` reads a node written under its
    /// context, its `@context` member not among its members.
    ///
    /// # Errors
    ///
    /// Returns the [`ElementRefusal`] of an evolution the model cannot hold,
    /// naming the element and the `id` it names, located in the evolution.
    pub fn temporal(&self, reading: ContextReading<'_>) -> Result<TemporalBody, Box<ElementRefusal>> {
        TemporalBody::from_node(reading.read(JsonPiece::new(&self.written))).map_err(|error| self.refusal(error))
    }

    /// The refusal of the element as what `error` says it could not be read as.
    fn refusal(&self, error: NodeReadError<Infallible>) -> Box<ElementRefusal> {
        match error {
            NodeReadError::Refused(refusal) => Box::new(ElementRefusal {
                index: self.index,
                id: self.id.clone(), // clone: the refusal outlives the entity it names
                reason: ElementRefusalReason::Refused(refusal),
            }),
            NodeReadError::Unreadable(never) => match never {},
        }
    }
}

/// The entities a peer answered with: `[..]`, or a single object, element by
/// element.
#[derive(Debug)]
pub struct ReceivedEntities(pub Vec<ReceivedElement<ContextualEntity>>);

impl ReceivedEntities {
    /// Read the entities of a received document from its text.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedDocumentError`] for text that is no JSON, or that is
    /// neither an array nor an object; a refused element is not an error.
    pub fn from_slice(text: &[u8]) -> Result<ReceivedEntities, ReceivedDocumentError> {
        let elements = received_elements(text)?;
        Ok(ReceivedEntities(
            elements
                .into_iter()
                .enumerate()
                .map(|(index, element)| contextual_entity(index, element))
                .collect(),
        ))
    }

    /// Read the one entity of a received document that answers with a single
    /// object, such as a Retrieve Entity (clause 5.7.1).
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedDocumentError`] for text that is no JSON, or that is
    /// not an object; a refused entity is not an error.
    pub fn from_entity_slice(text: &[u8]) -> Result<ReceivedEntities, ReceivedDocumentError> {
        match received_document(text)? {
            ReceivedDocument::Object(object) => Ok(ReceivedEntities(vec![contextual_entity(0, object)])),
            ReceivedDocument::Array(_) => Err(ReceivedDocumentError::ArrayForElement),
        }
    }

    /// Read the entities a notification carried in its `data` member (clause
    /// 5.3.1), already sliced out of the notification.
    #[must_use]
    pub fn from_elements(elements: Vec<JsonPiece<'_>>) -> ReceivedEntities {
        ReceivedEntities(
            elements
                .into_iter()
                .enumerate()
                .map(|(index, element)| contextual_entity(index, element))
                .collect(),
        )
    }
}

/// Read every element of a received array of `T`, each through `T`'s own
/// decoding.
#[must_use]
pub fn decoded_elements<T: DeserializeOwned>(elements: Vec<JsonPiece<'_>>) -> Vec<ReceivedElement<T>> {
    elements
        .into_iter()
        .enumerate()
        .map(|(index, element)| {
            let refused = |reason| {
                ReceivedElement::Refused(ElementRefusal {
                    index,
                    id: named_id(element),
                    reason,
                })
            };
            if let Some(inadmissible) = inadmissible_text(element) {
                return refused(inadmissible);
            }
            match serde_json::from_str::<T>(element.text()) {
                Ok(read) => ReceivedElement::Read(read),
                Err(error) => refused(ElementRefusalReason::Undecodable(error)),
            }
        })
        .collect()
}

/// The refusal of an element whose text a request's text would be refused for,
/// found in one streaming pass over it.
pub(crate) fn inadmissible_text(element: JsonPiece<'_>) -> Option<ElementRefusalReason> {
    admit_json_text(element.text().as_bytes()).err().map(ElementRefusalReason::Inadmissible)
}

/// A received document, by its outermost shape.
enum ReceivedDocument<'a> {
    /// An array, holding its elements.
    Array(Vec<JsonPiece<'a>>),
    /// One object.
    Object(JsonPiece<'a>),
}

/// The received document `text` is, by its outermost shape.
fn received_document(text: &[u8]) -> Result<ReceivedDocument<'_>, ReceivedDocumentError> {
    let raw: &RawValue = serde_json::from_slice(text).map_err(ReceivedDocumentError::Syntax)?;
    let document = JsonPiece::new(raw);
    match document.kind() {
        JsonKind::Array => Ok(ReceivedDocument::Array(document.elements().unwrap_or_default())),
        JsonKind::Object => Ok(ReceivedDocument::Object(document)),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => Err(ReceivedDocumentError::NotAnElementOrArray),
    }
}

/// The elements of a received document: those of an array, or the one object.
pub(crate) fn received_elements(text: &[u8]) -> Result<Vec<JsonPiece<'_>>, ReceivedDocumentError> {
    match received_document(text)? {
        ReceivedDocument::Array(elements) => Ok(elements),
        ReceivedDocument::Object(object) => Ok(vec![object]),
    }
}

/// One element read as a contextual entity.
fn contextual_entity(index: usize, element: JsonPiece<'_>) -> ReceivedElement<ContextualEntity> {
    let refused = |reason| {
        ReceivedElement::Refused(ElementRefusal {
            index,
            id: named_id(element),
            reason,
        })
    };
    if let Some(inadmissible) = inadmissible_text(element) {
        return refused(inadmissible);
    }
    let context = match element.member(keyword::CONTEXT).map(JsonPiece::to_literal).transpose() {
        Ok(context) => context,
        Err(error) => return refused(ElementRefusalReason::Context(error)),
    };
    ReceivedElement::Read(ContextualEntity {
        context,
        id: named_id(element),
        index,
        written: element.raw().to_owned(),
    })
}

/// The `id` an element names as `id` or `@id` (clause 4.4), when it names a
/// readable one.
pub(crate) fn named_id(element: JsonPiece<'_>) -> Option<Identifier> {
    ContextReading::keyword_spellings()
        .read(element)
        .member(keyword::ID_ALIAS)
        .and_then(CompactNode::text)
        .and_then(|id| id.parse().ok())
}

#[cfg(test)]
mod tests {
    use crate::{
        received::{ContextualEntity, ElementRefusalReason, ReceivedDocumentError, ReceivedElement, ReceivedEntities},
        temporal::temporal_body::TemporalBody,
    };
    use andromeda_ngsi_ld_representation::{
        compact_node::context_reading::ContextReading,
        container_terms::{ContainerTerms, TermContainer},
        core_term_aliases::CoreTermAliases,
    };

    fn read(text: &str) -> Vec<ReceivedElement<ContextualEntity>> {
        ReceivedEntities::from_slice(text.as_bytes()).expect("a document of entities").0
    }

    /// The entities of `elements`, each read under the keyword spellings, or
    /// the text of its refusal.
    fn entities(elements: &[ReceivedElement<ContextualEntity>]) -> Vec<Result<String, String>> {
        elements
            .iter()
            .map(|element| match element {
                ReceivedElement::Read(contextual) => contextual
                    .entity(ContextReading::keyword_spellings())
                    .map(|entity| entity.id.to_string())
                    .map_err(|refusal| refusal.to_string()),
                ReceivedElement::Refused(refusal) => Err(refusal.to_string()),
            })
            .collect()
    }

    const GOOD: &str = r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":50}}"#;

    #[test]
    fn an_array_of_entities_is_read_element_by_element() {
        let elements = read(&format!(r#"[{GOOD},{{"id":"urn:b","type":"T","type":"U"}},{GOOD}]"#));
        assert!(matches!(
            elements.as_slice(),
            [ReceivedElement::Read(_), ReceivedElement::Refused(_), ReceivedElement::Read(_)]
        ));
        let ReceivedElement::Refused(refusal) = &elements[1] else {
            panic!("the middle element is refused");
        };
        pretty_assertions::assert_eq!(refusal.index, 1);
        pretty_assertions::assert_eq!(refusal.id.as_ref().map(ToString::to_string), Some("urn:b".to_owned()));
    }

    /// An element is held as written until its context was processed, so an
    /// element that is no entity is refused only once it is read.
    #[test]
    fn an_element_that_is_no_entity_is_refused_when_read() {
        let elements = read(&format!(r#"[{GOOD},{{"id":"urn:b"}}]"#));
        let read = entities(&elements);
        pretty_assertions::assert_eq!(read[0], Ok("urn:ngsi-ld:Vehicle:1".to_owned()));
        assert!(read[1].is_err(), "{read:?}");
        let [_, ReceivedElement::Read(typeless)] = elements.as_slice() else {
            panic!("both elements are held: {elements:?}");
        };
        pretty_assertions::assert_eq!(typeless.id.as_ref().map(ToString::to_string), Some("urn:b".to_owned()));
    }

    /// Clause 4.4: a peer may spell `id` and `type` as `@id` and `@type`, at
    /// the entity and on an attribute alike.
    #[test]
    fn a_keyword_spelled_entity_reads_typed_and_names_its_id() {
        let elements = read(r#"[{"@id":"urn:ngsi-ld:Vehicle:9","@type":"Vehicle","speed":{"@type":"Property","value":1}}]"#);
        let [ReceivedElement::Read(contextual)] = elements.as_slice() else {
            panic!("one entity is held: {elements:?}");
        };
        pretty_assertions::assert_eq!(contextual.id.as_ref().map(ToString::to_string), Some("urn:ngsi-ld:Vehicle:9".to_owned()));
        let entity = contextual.entity(ContextReading::keyword_spellings()).expect("the entity reads");
        assert!(entity.attrs.contains_key("speed"), "{entity:?}");
    }

    #[test]
    fn a_single_entity_object_is_one_element() {
        assert!(matches!(read(GOOD).as_slice(), [ReceivedElement::Read(_)]));
    }

    #[test]
    fn an_entity_context_is_kept_beside_the_entity() {
        let elements = read(r#"[{"@context":"https://x/c.jsonld","id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"}]"#);
        let [ReceivedElement::Read(contextual)] = elements.as_slice() else {
            panic!("one entity is read: {elements:?}");
        };
        pretty_assertions::assert_eq!(contextual.context.as_ref().map(ToString::to_string), Some(r#""https://x/c.jsonld""#.to_owned()));
        let entity = contextual.entity(ContextReading::keyword_spellings()).expect("the entity reads");
        assert!(entity.attrs.is_empty(), "@context is no attribute");
    }

    /// A member the model cannot hold refuses the entity, naming the element,
    /// its `id` and the member, instead of being carried unread.
    #[test]
    fn an_unreadable_member_refuses_the_entity() {
        let read = entities(&read(r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property"}}]"#));
        let [Err(refusal)] = read.as_slice() else {
            panic!("the entity is refused: {read:?}");
        };
        assert!(refusal.contains("speed"), "{refusal}");
        assert!(refusal.contains("element 0 (urn:ngsi-ld:Vehicle:1)"), "the refusal names the entity: {refusal}");
    }

    /// Clause 4.3.6.8 has a broker reject invalid data and clause 6.3.17
    /// reports it with a warning: an element that names a member twice, at its
    /// top or deep inside an attribute, is refused alone, naming the member,
    /// and its neighbours are read.
    #[test]
    fn an_element_naming_a_member_twice_is_refused_alone_naming_it() {
        for (repeated, location) in [
            (r#"{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","type":"Car"}"#, "/type"),
            (
                r#"{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","speed":{"type":"Property","value":1,"value":2}}"#,
                "/speed/value",
            ),
            (
                r#"{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","speed":{"type":"Property","value":{"a":[{"b":1,"b":2}]}}}"#,
                "/speed/value/a/0/b",
            ),
        ] {
            let elements = read(&format!("[{GOOD},{repeated},{GOOD}]"));
            let [ReceivedElement::Read(_), ReceivedElement::Refused(refusal), ReceivedElement::Read(_)] = elements.as_slice() else {
                panic!("only the middle element is refused: {elements:?}");
            };
            assert!(matches!(refusal.reason, ElementRefusalReason::Inadmissible(_)), "{refusal}");
            assert!(refusal.to_string().contains(&format!("the member at {location} is named twice")), "{refusal}");
            pretty_assertions::assert_eq!(refusal.id.as_ref().map(ToString::to_string), Some("urn:ngsi-ld:Vehicle:2".to_owned()));
        }
    }

    #[test]
    fn exact_numbers_survive() {
        let elements = read(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":1.10}}"#);
        let [ReceivedElement::Read(contextual)] = elements.as_slice() else {
            panic!("one entity is read");
        };
        let entity = contextual.entity(ContextReading::keyword_spellings()).expect("the entity reads");
        let written = serde_json::to_string(&entity).expect("the entity writes");
        assert!(written.contains("1.1"), "{written}");
    }

    /// The one element of `text`, held as written.
    fn held(text: &str) -> ContextualEntity {
        let mut elements = read(text);
        let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
            panic!("one element is held: {elements:?}");
        };
        contextual
    }

    /// The evolution of `text`, read under the keyword spellings, or the text
    /// of its refusal.
    fn evolution(text: &str) -> Result<TemporalBody, String> {
        held(text).temporal(ContextReading::keyword_spellings()).map_err(|refusal| refusal.to_string())
    }

    /// How many instances `body` holds.
    fn instances(body: &TemporalBody) -> usize {
        match body {
            TemporalBody::Evolution(entity) => entity.attribute_members().map(|(_, attribute)| attribute.instances().len()).sum(),
            TemporalBody::Aggregated(_) => panic!("an evolution, not aggregates: {body:?}"),
        }
    }

    /// The aliases a source context defining `kind` for `@type` and `vals` for
    /// `ngsi-ld:hasValues` gives.
    fn source_aliases() -> CoreTermAliases {
        let mut aliases = CoreTermAliases::keyword_spellings().clone();
        aliases.alias("kind", "type");
        aliases.alias("vals", "values");
        aliases
    }

    const EVOLUTION: &str = r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","value":50,"observedAt":"2020-01-01T00:00:00Z","instanceId":"urn:ngsi-ld:i:1"}]}"#;

    #[test]
    fn a_temporal_evolution_reads_its_instances() {
        let body = evolution(EVOLUTION).expect("the evolution reads");
        pretty_assertions::assert_eq!(instances(&body), 1);
    }

    /// Clause 4.5.1: an evolution without `type` is refused, naming it.
    #[test]
    fn an_evolution_without_type_is_refused() {
        let refusal = evolution(r#"{"id":"urn:a","speed":[{"type":"Property","value":1}]}"#).expect_err("no type");
        assert!(refusal.contains("type"), "{refusal}");
    }

    /// An instance naming no value member of its family is refused rather than
    /// read as a Property with a null value.
    #[test]
    fn an_instance_without_a_family_member_is_refused() {
        let refusal = evolution(r#"{"id":"urn:a","type":"T","speed":[{"type":"Property","observedAt":"2020-01-01T00:00:00Z"}]}"#).expect_err("no value");
        assert!(refusal.contains("speed"), "{refusal}");
    }

    /// A peer answering an aggregated query answers with aggregates.
    #[test]
    fn an_aggregated_answer_is_read_as_aggregates() {
        let body = evolution(r#"{"id":"urn:a","type":"T","w":{"type":"Property","avg":[[1.5,"2026-01-01T00:00:00Z","2026-01-01T01:00:00Z"]]}}"#)
            .expect("aggregates read");
        assert!(matches!(body, TemporalBody::Aggregated(_)), "{body:?}");
    }

    /// Clauses 4.4 and 5.5.4: an instance whose `type` is spelled by an alias
    /// of `@type` the source context defines reads as the Property it is.
    #[test]
    fn an_evolution_under_an_alias_of_type_reads_its_instances() {
        let aliases = source_aliases();
        let containers = ContainerTerms::none();
        let contextual =
            held(r#"{"id":"urn:ngsi-ld:Vehicle:1","kind":"Vehicle","speed":[{"kind":"Property","value":20,"observedAt":"2026-01-01T00:00:00Z"}]}"#);
        let body = contextual.temporal(ContextReading::new(&aliases, &containers)).expect("the evolution reads");
        pretty_assertions::assert_eq!(instances(&body), 1);
        assert!(
            contextual.temporal(ContextReading::keyword_spellings()).is_err(),
            "kind is no type without the source context"
        );
    }

    /// Clause 4.5.9: the simplified temporal representation written with an
    /// alias of the `values` plural reads as the instances it lists.
    #[test]
    fn an_evolution_under_an_alias_of_a_temporal_plural_reads_its_instances() {
        let aliases = source_aliases();
        let containers = ContainerTerms::none();
        let contextual = held(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"kind":"Property","vals":[[20,"2026-01-01T00:00:00Z"],[21,"2026-01-01T01:00:00Z"]]}}"#,
        );
        let body = contextual.temporal(ContextReading::new(&aliases, &containers)).expect("the evolution reads");
        pretty_assertions::assert_eq!(instances(&body), 2);
    }

    /// JSON-LD 1.1 section 4.6: a `@set` alias of `@type` writes the type as an
    /// array of one, which reads as the type it holds.
    #[test]
    fn an_evolution_under_a_set_container_reads_its_instances() {
        let aliases = source_aliases();
        let mut containers = ContainerTerms::none();
        containers.contain("kind", TermContainer::Set);
        let contextual =
            held(r#"{"id":"urn:ngsi-ld:Vehicle:1","kind":["Vehicle"],"speed":[{"kind":["Property"],"value":20,"observedAt":"2026-01-01T00:00:00Z"}]}"#);
        let body = contextual.temporal(ContextReading::new(&aliases, &containers)).expect("the evolution reads");
        pretty_assertions::assert_eq!(instances(&body), 1);
    }

    /// A Core API answer reads through the containers of its context too.
    #[test]
    fn an_entity_under_a_set_container_reads_its_attribute() {
        let aliases = source_aliases();
        let mut containers = ContainerTerms::none();
        containers.contain("kind", TermContainer::Set);
        let contextual = held(r#"{"id":"urn:ngsi-ld:Vehicle:1","kind":["Vehicle"],"speed":{"kind":["Property"],"value":20}}"#);
        let entity = contextual.entity(ContextReading::new(&aliases, &containers)).expect("the entity reads");
        assert!(entity.attrs.contains_key("speed"), "{entity:?}");
    }

    #[test]
    fn a_document_that_is_not_json_is_an_error() {
        assert!(matches!(ReceivedEntities::from_slice(b"<html>"), Err(ReceivedDocumentError::Syntax(_))));
        assert!(matches!(ReceivedEntities::from_slice(b"5"), Err(ReceivedDocumentError::NotAnElementOrArray)));
    }

    #[test]
    fn a_single_entity_document_reads_its_object() {
        let ReceivedEntities(elements) = ReceivedEntities::from_entity_slice(br#"{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}"#).expect("one entity");
        pretty_assertions::assert_eq!(entities(&elements), vec![Ok("urn:ngsi-ld:Vehicle:1".to_owned())]);
    }

    /// Clause 5.7.1: a retrieval answers one Entity, so an array is no answer
    /// to it, even one holding the requested entity alone.
    #[test]
    fn a_single_entity_document_refuses_an_array() {
        assert!(matches!(
            ReceivedEntities::from_entity_slice(br#"[{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}]"#),
            Err(ReceivedDocumentError::ArrayForElement)
        ));
        assert!(matches!(
            ReceivedEntities::from_entity_slice(b"5"),
            Err(ReceivedDocumentError::NotAnElementOrArray)
        ));
    }
}
