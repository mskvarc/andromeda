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
//! term of it (clause 5.5.7, JSON-LD 1.1 API section 6.2.2), so an entity is
//! held as written and read into the model only once the context it was
//! written in was processed, through the aliases of that context.

use crate::{
    entity::Entity,
    node_refusal::{NodeReadError, RefusedNode},
};
use andromeda_json_literal::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_number::json_value::text_admission::{JsonTextRefusal, admit_json_text};
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, aliased_node::AliasedNode},
    core_term_aliases::CoreTermAliases,
};
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
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
#[error("element {index}{} is refused: {reason}", .id.as_ref().map(|id| format!(" ({id})")).unwrap_or_default())]
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
    /// number outside the supported range or a member named like the JSON
    /// reader's own tokens. A broker "reject[s] data that is invalid"
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
    /// The entity as written, its own `@context` among its members.
    written: Box<RawValue>,
}

impl ContextualEntity {
    /// The entity, each member read as the core term `aliases` say it stands
    /// for, its `@context` member not among its members.
    ///
    /// # Errors
    ///
    /// Returns the [`RefusedNode`] of an entity the model cannot hold, located
    /// in the entity.
    pub fn entity(&self, aliases: &CoreTermAliases) -> Result<Entity, RefusedNode> {
        match Entity::from_node(AliasedNode::new(JsonPiece::new(&self.written), aliases)) {
            Ok(entity) => Ok(entity),
            Err(NodeReadError::Refused(refusal)) => Err(refusal),
            Err(NodeReadError::Unreadable(never)) => match never {},
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

/// The elements of a received document: those of an array, or the one object.
pub(crate) fn received_elements(text: &[u8]) -> Result<Vec<JsonPiece<'_>>, ReceivedDocumentError> {
    let raw: &RawValue = serde_json::from_slice(text).map_err(ReceivedDocumentError::Syntax)?;
    let document = JsonPiece::new(raw);
    match document.kind() {
        JsonKind::Array => Ok(document.elements().unwrap_or_default()),
        JsonKind::Object => Ok(vec![document]),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => Err(ReceivedDocumentError::NotAnElementOrArray),
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
        written: element.raw().to_owned(),
    })
}

/// The `id` an element names as `id` or `@id` (clause 4.4), when it names a
/// readable one.
pub(crate) fn named_id(element: JsonPiece<'_>) -> Option<Identifier> {
    AliasedNode::new(element, CoreTermAliases::keyword_spellings())
        .member(keyword::ID_ALIAS)
        .and_then(CompactNode::text)
        .and_then(|id| id.parse().ok())
}

#[cfg(test)]
mod tests {
    use crate::received::{ContextualEntity, ElementRefusalReason, ReceivedDocumentError, ReceivedElement, ReceivedEntities};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;

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
                    .entity(CoreTermAliases::keyword_spellings())
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
        let entity = contextual.entity(CoreTermAliases::keyword_spellings()).expect("the entity reads");
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
        let entity = contextual.entity(CoreTermAliases::keyword_spellings()).expect("the entity reads");
        assert!(entity.attrs.is_empty(), "@context is no attribute");
    }

    /// A member the model cannot hold refuses the entity, naming
    /// the member, instead of being carried unread.
    #[test]
    fn an_unreadable_member_refuses_the_entity() {
        let read = entities(&read(r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property"}}]"#));
        let [Err(refusal)] = read.as_slice() else {
            panic!("the entity is refused: {read:?}");
        };
        assert!(refusal.contains("speed"), "{refusal}");
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
        let entity = contextual.entity(CoreTermAliases::keyword_spellings()).expect("the entity reads");
        let written = serde_json::to_string(&entity).expect("the entity writes");
        assert!(written.contains("1.1"), "{written}");
    }

    #[test]
    fn a_document_that_is_not_json_is_an_error() {
        assert!(matches!(ReceivedEntities::from_slice(b"<html>"), Err(ReceivedDocumentError::Syntax(_))));
        assert!(matches!(ReceivedEntities::from_slice(b"5"), Err(ReceivedDocumentError::NotAnElementOrArray)));
    }
}
