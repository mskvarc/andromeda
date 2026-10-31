//! The Building of structured attributes that the q-language tests query: language maps, value lists, a list
//! relationship and a vocabulary property, in the spellings the broker must select on (clause 4.9).
//!
//! The model writes a `ListRelationship` as objects and refuses a `languageMap` entry holding several strings, so
//! `roomLinks` and `roomAliases`, which the tests send as plain strings and arrays, go out as written beside the typed
//! members.

use crate::{broker::BrokerClient, namespace::Namespace};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{UncheckedDocument, UncheckedRequest, operation::CreateEntity},
    model::{
        BuildError,
        CoreDatatype,
        Entity,
        EntityId,
        EntityTypeName,
        Input,
        InvalidJson,
        LanguageMap,
        LanguageMapError,
        LanguageProperty,
        ListProperty,
        Lossless,
        NameError,
        Property,
        RawJson,
        TypedValue,
        TypedValueError,
        Value,
        VocabProperty,
        VocabTerm,
    },
};

/// Why the Building could not be built or its request not written.
#[derive(Debug, thiserror::Error)]
pub enum RoomBuildingError {
    /// The entity is not valid.
    #[error("the entity could not be built")]
    Entity(#[from] BuildError),
    /// A member written as text is not JSON.
    #[error("a member is not JSON")]
    Json(#[from] InvalidJson),
    /// A language map is not valid.
    #[error("a language map is not valid")]
    Language(#[from] LanguageMapError),
    /// A vocabulary term is not valid.
    #[error("a vocabulary term is not valid")]
    Term(#[from] NameError),
    /// A typed value is not a lexical form of its datatype.
    #[error("a typed value is not valid")]
    Typed(#[from] TypedValueError),
    /// The client refused to write the request.
    #[error("the request could not be written")]
    Request(#[source] Box<ClientError<ReqwestError>>),
}

/// How `roomInstants` holds its instants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomInstants {
    /// Two strings spelling a fractional and a whole-second `DateTime`.
    Strings,
    /// Two JSON-LD `DateTime` values, with the strings of the same text under `roomInstantLabels`.
    Typed,
}

/// The Building as the entity the model writes and the document that also carries the members it cannot.
#[derive(Debug)]
pub struct RoomBuilding {
    /// The entity with every member the model writes.
    entity: Entity<Input>,
    /// The entity's members plus `roomAliases` and `roomLinks` as written.
    document: UncheckedDocument,
}

impl RoomBuilding {
    /// The Building named `id` of type `entity_type`.
    ///
    /// # Errors
    ///
    /// Returns [`RoomBuildingError`] when a member is not valid.
    pub fn new(id: &EntityId, entity_type: &EntityTypeName, namespace: &Namespace, instants: RoomInstants) -> Result<RoomBuilding, RoomBuildingError> {
        let instant_values = match instants {
            RoomInstants::Strings => vec![Value::from("2024-01-01T00:00:00.250Z"), Value::from("2024-01-02T00:00:00Z")],
            RoomInstants::Typed => vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-01T00:00:00.25Z")?),
                Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-02T00:00:00Z")?),
            ],
        };
        let mut builder = Entity::builder()
            .id(id.clone())
            .entity_type(entity_type.clone())
            .language_property(
                "roomLabel",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Tuer")])?),
            )
            .list_property("roomTags", ListProperty::builder(vec![Value::from("residential"), Value::from("commercial")]))
            .list_property("roomInstants", ListProperty::builder(instant_values));
        if instants == RoomInstants::Typed {
            builder = builder.list_property(
                "roomInstantLabels",
                ListProperty::builder(vec![Value::from("2024-01-01T00:00:00.250Z"), Value::from("2024-01-02T00:00:00Z")]),
            );
        }
        let entity = builder
            .list_property("roomTimes", ListProperty::builder(vec![Value::from("09:30:00.250Z"), Value::from("13:00:00Z")]))
            .list_property("roomSpeeds", ListProperty::builder(vec![Value::from(1), Value::from(2), Value::from(3)]))
            .property("roomTemp", Property::builder(20))
            .property("roomName", Property::builder("atrium"))
            .property("roomExtras", Property::builder(Value::from(vec!["a", "b", "c"])))
            .vocab_property(
                "roomVocab",
                VocabProperty::builder(vec![
                    "http://example.org/Male".parse::<VocabTerm>()?,
                    "http://example.org/Other".parse::<VocabTerm>()?,
                ]),
            )
            .build()?;
        let aliases: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"fr": ["chat", "rouge"], "en": ["red", "cat"]}}"#.parse()?;
        let links: RawJson =
            format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-1", "urn:ngsi-ld:Room:{namespace}-2"]}}"#).parse()?;
        let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized)
            .set_member("roomAliases", &aliases)
            .set_member("roomLinks", &links);
        Ok(RoomBuilding { entity, document })
    }

    /// The Create Entity request carrying the document.
    ///
    /// # Errors
    ///
    /// Returns [`RoomBuildingError::Request`] when the client cannot write it.
    pub fn create_request(&self, broker: &BrokerClient) -> Result<UncheckedRequest, RoomBuildingError> {
        let request = broker
            .unchecked(&CreateEntity::builder().entity(&self.entity).build())
            .map_err(|error| RoomBuildingError::Request(Box::new(error)))?;
        Ok(request.document(&self.document))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        room_building::{RoomBuilding, RoomInstants},
    };
    use googletest::prelude::*;

    #[gtest]
    fn the_document_carries_the_members_the_model_cannot_write() -> Result<()> {
        let namespace = Namespace::fresh();
        let id = namespace.urn("Building", 1)?;
        let entity_type = namespace.entity_type("Building")?;
        let room = RoomBuilding::new(&id, &entity_type, &namespace, RoomInstants::Strings)?;
        let text = room.document.to_string();
        verify_that!(
            text,
            all!(
                contains_substring("\"rouge\""),
                contains_substring("\"objectList\":[\"urn:ngsi-ld:Room:"),
                contains_substring(r#""roomVocab":{"type":"VocabProperty","vocab":["http://example.org/Male","http://example.org/Other"]}"#)
            )
        )
    }

    #[gtest]
    fn typed_instants_add_the_label_list() -> Result<()> {
        let namespace = Namespace::fresh();
        let id = namespace.urn("Building", 1)?;
        let entity_type = namespace.entity_type("Building")?;
        let room = RoomBuilding::new(&id, &entity_type, &namespace, RoomInstants::Typed)?;
        let text = room.document.to_string();
        verify_that!(
            text,
            all!(contains_substring("roomInstantLabels"), contains_substring("\"@type\":\"DateTime\""))
        )
    }
}
