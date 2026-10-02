//! Reading a compacted entity document as the compact [`Entity`].
//!
//! Compaction writes the entity's identifier and types under the context's
//! aliases of `@id` and `@type`; the compact model reads them as `id` and
//! `type`. The document is reshaped accordingly, named after the identifier
//! the caller read it by when that differs from its own, and decoded exactly
//! as a request document is decoded.

use crate::compaction_error::CompactionError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use serde_json::{Map, Value};

/// The keyword spelling of the identifier member.
const ID_KEYWORD: &str = "@id";
/// The keyword spelling of the types member.
const TYPE_KEYWORD: &str = "@type";
/// The member the compact model reads the identifier from.
const ID_MEMBER: &str = "id";
/// The member the compact model reads the types from.
const TYPE_MEMBER: &str = "type";

/// Decode a compacted entity document, naming it `id_override` when given.
///
/// # Errors
///
/// Returns [`CompactionError::NotAnObject`] for a document that holds no
/// entity object, [`CompactionError::MissingId`] for an entity without an
/// identifier, and [`CompactionError::Decode`] when the compact model refuses
/// the document.
pub(crate) fn decode_entity(compacted: Value, id_override: Option<&Identifier>) -> Result<Entity, CompactionError> {
    let mut object = entity_object(compacted).ok_or(CompactionError::NotAnObject)?;
    if let Some(types) = object.remove(TYPE_KEYWORD) {
        object.insert(TYPE_MEMBER.to_owned(), types);
    }
    let id = id_override
        .map(|id| id.as_str().to_owned())
        .or_else(|| object.get(ID_MEMBER).and_then(Value::as_str).map(str::to_owned))
        .or_else(|| object.get(ID_KEYWORD).and_then(Value::as_str).map(str::to_owned))
        .ok_or(CompactionError::MissingId)?;
    object.remove(ID_KEYWORD);
    object.insert(ID_MEMBER.to_owned(), Value::String(id));
    deserialize_exact(&Value::Object(object)).map_err(CompactionError::Decode)
}

/// The entity object of a compacted document: the document itself, or the
/// first item of an array.
fn entity_object(compacted: Value) -> Option<Map<String, Value>> {
    match compacted {
        Value::Array(mut items) if !items.is_empty() => match items.swap_remove(0) {
            Value::Object(object) => Some(object),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
        },
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{compacted_entity::decode_entity, compaction_error::CompactionError};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::json;

    #[test]
    fn keyword_members_are_read_as_the_compact_id_and_type() {
        let entity = decode_entity(json!({"@id": "urn:ngsi-ld:A:1", "@type": "A"}), None).expect("an entity");
        pretty_assertions::assert_eq!(entity.id.as_str(), "urn:ngsi-ld:A:1");
    }

    #[test]
    fn an_override_names_the_entity_it_was_read_by() {
        let id: Identifier = "urn:ngsi-ld:A:2".parse().expect("an identifier");
        let entity = decode_entity(json!([{"id": "urn:ngsi-ld:A:1", "type": "A"}]), Some(&id)).expect("an entity");
        pretty_assertions::assert_eq!(entity.id, id);
    }

    #[test]
    fn a_document_without_an_object_or_an_id_is_refused() {
        assert!(matches!(decode_entity(json!([]), None), Err(CompactionError::NotAnObject)));
        assert!(matches!(decode_entity(json!({"type": "A"}), None), Err(CompactionError::MissingId)));
        assert!(matches!(decode_entity(json!({"id": "urn:ngsi-ld:A:1"}), None), Err(CompactionError::Decode(_))));
    }
}
