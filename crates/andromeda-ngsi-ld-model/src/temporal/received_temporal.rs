//! The Temporal Evolutions a context source answered a temporal operation with
//! (clauses 5.7.3.4, 5.7.4.4 and 6.3.17).
//!
//! The answer is read as [`crate::received`] reads an answer of entities: an
//! array element by element, or the one object a retrieval answers with, each
//! element from the slice of text it is, so an element the model cannot hold
//! is reported and skipped instead of failing the page, and no JSON tree is
//! built on the way. A Temporal Evolution is read on receipt, since the walk
//! over the answer's pages counts its instances, under the clause 4.4 keyword
//! spellings every context reads.

use crate::{
    node_refusal::NodeReadError,
    received::{ElementRefusal, ElementRefusalReason, ReceivedDocumentError, ReceivedElement, inadmissible_text, named_id, received_elements},
    temporal::temporal_body::TemporalBody,
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_representation::{compact_node::aliased_node::AliasedNode, core_term_aliases::CoreTermAliases};
use andromeda_ngsi_ld_vocabulary::keyword;

/// A Temporal Evolution as a peer wrote it, with the `@context` it named for
/// itself (JSON-LD 1.1: a node object's own context applies to that node).
#[derive(Debug)]
pub struct ContextualTemporalEntity {
    /// The entity's own `@context`, when it named one.
    pub context: Option<JsonLiteral>,
    /// The Temporal Evolution, its `@context` member not among its members:
    /// the instances of its attributes, or the aggregates over them.
    pub body: TemporalBody,
}

/// The Temporal Evolutions a peer answered with: `[..]`, or a single object,
/// element by element.
#[derive(Debug)]
pub struct ReceivedTemporalEntities(pub Vec<ReceivedElement<ContextualTemporalEntity>>);

impl ReceivedTemporalEntities {
    /// Read the Temporal Evolutions of a received document from its text.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedDocumentError`] for text that is no JSON, or that is
    /// neither an array nor an object; a refused element is not an error.
    pub fn from_slice(text: &[u8]) -> Result<ReceivedTemporalEntities, ReceivedDocumentError> {
        let elements = received_elements(text)?;
        Ok(ReceivedTemporalEntities(
            elements
                .into_iter()
                .enumerate()
                .map(|(index, element)| contextual_temporal_entity(index, element))
                .collect(),
        ))
    }
}

/// One element read as a contextual Temporal Evolution.
fn contextual_temporal_entity(index: usize, element: JsonPiece<'_>) -> ReceivedElement<ContextualTemporalEntity> {
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
    match TemporalBody::from_node(AliasedNode::new(element, CoreTermAliases::keyword_spellings())) {
        Ok(body) => ReceivedElement::Read(ContextualTemporalEntity { context, body }),
        Err(error) => refused(match error {
            NodeReadError::Refused(refusal) => ElementRefusalReason::Refused(refusal),
            NodeReadError::Unreadable(never) => match never {},
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        received::{ElementRefusalReason, ReceivedElement},
        temporal::{
            received_temporal::{ContextualTemporalEntity, ReceivedTemporalEntities},
            temporal_body::TemporalBody,
        },
    };

    fn read(text: &str) -> Vec<ReceivedElement<ContextualTemporalEntity>> {
        ReceivedTemporalEntities::from_slice(text.as_bytes())
            .expect("a document of temporal entities")
            .0
    }

    const GOOD: &str = r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","value":50,"observedAt":"2020-01-01T00:00:00Z","instanceId":"urn:ngsi-ld:i:1"}]}"#;

    /// A remote answer is read element by element, a refused one skipped.
    #[test]
    fn an_array_of_temporal_entities_is_read_element_by_element() {
        let elements = read(&format!(r#"[{GOOD},{{"id":"urn:b"}},{GOOD}]"#));
        assert!(matches!(
            elements.as_slice(),
            [ReceivedElement::Read(_), ReceivedElement::Refused(_), ReceivedElement::Read(_)]
        ));
    }

    /// Clause 4.5.1: a remote answer without `type` is refused, naming it.
    #[test]
    fn a_remote_answer_without_type_is_refused() {
        let elements = read(r#"{"id":"urn:a","speed":[{"type":"Property","value":1}]}"#);
        let [ReceivedElement::Refused(refusal)] = elements.as_slice() else {
            panic!("the answer is refused: {elements:?}");
        };
        assert!(matches!(refusal.reason, ElementRefusalReason::Refused(_)));
        assert!(refusal.to_string().contains("type"), "{refusal}");
    }

    /// Clause 4.3.6.8: a peer's history that names an instance member twice is
    /// invalid data, refused alone.
    #[test]
    fn a_remote_answer_naming_a_member_twice_is_refused() {
        let elements = read(&format!(
            r#"[{GOOD},{{"id":"urn:a","type":"T","speed":[{{"type":"Property","value":1,"value":2,"observedAt":"2020-01-01T00:00:00Z"}}]}}]"#
        ));
        let [ReceivedElement::Read(_), ReceivedElement::Refused(refusal)] = elements.as_slice() else {
            panic!("only the second answer is refused: {elements:?}");
        };
        assert!(matches!(refusal.reason, ElementRefusalReason::Inadmissible(_)), "{refusal}");
        assert!(refusal.to_string().contains("the member at /speed/0/value is named twice"), "{refusal}");
    }

    /// An instance naming no value member of its family is refused rather than
    /// read as a Property with a null value.
    #[test]
    fn an_instance_without_a_family_member_is_refused() {
        let elements = read(r#"{"id":"urn:a","type":"T","speed":[{"type":"Property","observedAt":"2020-01-01T00:00:00Z"}]}"#);
        let [ReceivedElement::Refused(refusal)] = elements.as_slice() else {
            panic!("the answer is refused: {elements:?}");
        };
        assert!(refusal.to_string().contains("speed"), "{refusal}");
    }

    /// A peer answering an aggregated query answers with aggregates, which are
    /// read as such rather than refused.
    #[test]
    fn an_aggregated_answer_is_read_as_aggregates() {
        let elements = read(r#"[{"id":"urn:a","type":"T","w":{"type":"Property","avg":[[1.5,"2026-01-01T00:00:00Z","2026-01-01T01:00:00Z"]]}}]"#);
        let [ReceivedElement::Read(contextual)] = elements.as_slice() else {
            panic!("one entity is read: {elements:?}");
        };
        assert!(matches!(contextual.body, TemporalBody::Aggregated(_)));
    }

    /// The entity's own `@context` is kept beside it, and is no attribute.
    #[test]
    fn an_entity_context_is_kept_beside_the_temporal_entity() {
        let elements = read(r#"{"@context":"https://x/c.jsonld","id":"urn:a","type":"T"}"#);
        let [ReceivedElement::Read(contextual)] = elements.as_slice() else {
            panic!("one entity is read: {elements:?}");
        };
        assert!(contextual.context.is_some());
        assert!(matches!(&contextual.body, TemporalBody::Evolution(entity) if entity.attrs.is_empty()));
    }
}
