//! Rewording a compact document from one context's terms into another's
//! (clause 4.3.6.6).
//!
//! A forwarded request is written in its forwarding context
//! ([`crate::forwarding_context`]) and its source answers in that context.
//! When the forwarding context is the inbound request's own, both directions
//! are already in the client's terms. When it is another one -- the
//! registration's `jsonldContext`, or the core context standing in for a
//! compound inbound context -- every name of the request body is spelled in
//! the forwarding context, and every name of the answer is read back in the
//! inbound one. Both directions are the same operation: each name is expanded
//! against the context the document is written in and compacted against the
//! one it is to be read in, while the document's shape, identifiers and values
//! stay as written. No document is expanded as a whole for it.
//!
//! What counts as a name depends on the kind of document: an entity (or a
//! temporal entity) names its type and its attributes, an attribute names its
//! sub-attributes and what its members name, and a discovery answer (clauses
//! 5.7.5 to 5.7.10) names entity types and attributes in its list members.
//! Core members keep their spelling in every context, because the core context
//! has absolute precedence (clause 5.5.7).

use crate::{error::FederationError, federation_invariant::FederationInvariant, forwarding_context::Rewording};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, is_core_term};
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin, str::FromStr};

/// The members of a discovery answer that hold entity type names.
const TYPE_NAME_MEMBERS: [&str; 3] = ["typeList", "typeNames", "typeName"];
/// The members of a discovery answer that hold attribute names.
const ATTRIBUTE_NAME_MEMBERS: [&str; 3] = ["attributeList", "attributeNames", "attributeName"];
/// The members of a discovery answer that hold nested discovery objects.
const NESTED_DISCOVERY_MEMBERS: [&str; 1] = ["attributeDetails"];
/// The entity members that are not attributes.
const ENTITY_IDENTITY_MEMBERS: [&str; 3] = ["id", "type", "@context"];

/// What kind of document is reworded, which decides where its names sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentVocabulary {
    /// An entity, an entity fragment, a temporal entity, or an array of them
    /// (clauses 5.6 and 5.7.1 to 5.7.4).
    Entities,
    /// The one attribute, or attribute instance, a request path names
    /// (clauses 5.6.4, 5.6.14 and 5.6.19).
    Attribute,
    /// A type or attribute discovery answer (clauses 5.7.5 to 5.7.10).
    Discovery,
    /// A document that carries no names, such as an `EntityMap` (clause 5.14).
    Opaque,
}

/// Rewords one compact document from the context it is written in into the
/// context it is to be read in.
pub(crate) struct DocumentRewording<'a> {
    /// The engine that expands.
    engine: &'a JsonLdEngine,
    /// The context the document is written in, prepared for term lookups.
    written_in: PreparedTermContext,
    /// Compacts names against the context the document is to be read in.
    read_in: NameCompaction,
}

impl<'a> DocumentRewording<'a> {
    /// `document`, a `vocabulary` document written in `written_in`, with every
    /// name it carries spelled in `read_in`; unchanged when `rewording` says
    /// the two contexts are one.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when `read_in` does not resolve or
    /// a name in the document does not expand against `written_in`, and
    /// [`FederationInvariant::ForwardedName`] when a reworded spelling is not an
    /// NGSI-LD name.
    pub(crate) async fn reword(
        engine: &'a JsonLdEngine,
        written_in: &ContextSpec,
        read_in: &ContextSpec,
        document: Value,
        vocabulary: DocumentVocabulary,
        rewording: Rewording,
    ) -> Result<Value, FederationError> {
        match (rewording, vocabulary) {
            (Rewording::AsInbound, _) | (Rewording::Reword, DocumentVocabulary::Opaque) => Ok(document),
            (Rewording::Reword, DocumentVocabulary::Entities | DocumentVocabulary::Attribute | DocumentVocabulary::Discovery) => {
                let rewording = DocumentRewording {
                    engine,
                    written_in: engine.prepare_term_context(written_in),
                    read_in: NameCompaction::new(engine.compactor(read_in).processed_context().await?),
                };
                match vocabulary {
                    DocumentVocabulary::Entities => rewording.entities(document).await,
                    DocumentVocabulary::Attribute => rewording.attribute(document).await,
                    DocumentVocabulary::Discovery => rewording.discovery(document).await,
                    DocumentVocabulary::Opaque => Ok(document),
                }
            }
        }
    }

    /// Reword an entity or an array of them.
    async fn entities(&self, document: Value) -> Result<Value, FederationError> {
        match document {
            Value::Array(entities) => {
                let mut reworded = Vec::with_capacity(entities.len());
                for entity in entities {
                    reworded.push(self.entity(entity).await?);
                }
                Ok(Value::Array(reworded))
            }
            entity @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => self.entity(entity).await,
        }
    }

    /// Reword one entity: its type names, its attribute names and everything
    /// its attributes name.
    fn entity(&self, entity: Value) -> Pin<Box<dyn Future<Output = Result<Value, FederationError>> + Send + '_>> {
        Box::pin(async move {
            let Value::Object(members) = entity else {
                return Ok(entity);
            };
            let mut reworded = Map::with_capacity(members.len());
            for (name, value) in members {
                if name == "type" {
                    reworded.insert(name, self.type_names(value).await?);
                } else if ENTITY_IDENTITY_MEMBERS.contains(&name.as_str()) {
                    reworded.insert(name, value);
                } else {
                    let attribute = self.attribute_name(&name).await?.to_string();
                    reworded.insert(attribute, self.attribute(value).await?);
                }
            }
            Ok(Value::Object(reworded))
        })
    }

    /// Reword one attribute, an array of its instances, or its datasets.
    fn attribute(&self, attribute: Value) -> Pin<Box<dyn Future<Output = Result<Value, FederationError>> + Send + '_>> {
        Box::pin(async move {
            match attribute {
                Value::Array(instances) => {
                    let mut reworded = Vec::with_capacity(instances.len());
                    for instance in instances {
                        reworded.push(self.attribute(instance).await?);
                    }
                    Ok(Value::Array(reworded))
                }
                Value::Object(members) => {
                    let mut reworded = Map::with_capacity(members.len());
                    for (name, value) in members {
                        let value = self.attribute_member(&name, value).await?;
                        let name = if is_core_term(&name) {
                            name
                        } else {
                            self.attribute_name(&name).await?.to_string()
                        };
                        reworded.insert(name, value);
                    }
                    Ok(Value::Object(reworded))
                }
                scalar @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => Ok(scalar),
            }
        })
    }

    /// Reword one member of an attribute according to what it holds.
    async fn attribute_member(&self, name: &str, value: Value) -> Result<Value, FederationError> {
        if name == compact_properties::VALUE || name == compact_properties::VALUE_LIST {
            return self.compound_value(value).await;
        }
        if name == compact_properties::VOCAB || name == compact_properties::OBJECT_TYPE {
            return self.type_names(value).await;
        }
        if name == compact_properties::ENTITY {
            return self.entity(value).await;
        }
        if name == compact_properties::ENTITY_LIST {
            return self.entities(value).await;
        }
        if is_core_term(name) {
            return Ok(value);
        }
        // A member no core term names is a sub-attribute.
        self.attribute(value).await
    }

    /// Reword the member names of a structured Property value, which JSON-LD
    /// expands as terms (clause 4.5.2).
    fn compound_value(&self, value: Value) -> Pin<Box<dyn Future<Output = Result<Value, FederationError>> + Send + '_>> {
        Box::pin(async move {
            match value {
                Value::Array(items) => {
                    let mut reworded = Vec::with_capacity(items.len());
                    for item in items {
                        reworded.push(self.compound_value(item).await?);
                    }
                    Ok(Value::Array(reworded))
                }
                Value::Object(members) => {
                    let mut reworded = Map::with_capacity(members.len());
                    for (name, member) in members {
                        let name = if is_core_term(&name) {
                            name
                        } else {
                            self.attribute_name(&name).await?.to_string()
                        };
                        reworded.insert(name, self.compound_value(member).await?);
                    }
                    Ok(Value::Object(reworded))
                }
                scalar @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => Ok(scalar),
            }
        })
    }

    /// Reword a discovery answer: its list members of type and attribute names,
    /// and the discovery objects nested in it.
    fn discovery(&self, answer: Value) -> Pin<Box<dyn Future<Output = Result<Value, FederationError>> + Send + '_>> {
        Box::pin(async move {
            match answer {
                Value::Array(items) => {
                    let mut reworded = Vec::with_capacity(items.len());
                    for item in items {
                        reworded.push(self.discovery(item).await?);
                    }
                    Ok(Value::Array(reworded))
                }
                Value::Object(members) => {
                    let mut reworded = Map::with_capacity(members.len());
                    for (name, value) in members {
                        let value = if TYPE_NAME_MEMBERS.contains(&name.as_str()) {
                            self.type_names(value).await?
                        } else if ATTRIBUTE_NAME_MEMBERS.contains(&name.as_str()) {
                            self.attribute_names(value).await?
                        } else if NESTED_DISCOVERY_MEMBERS.contains(&name.as_str()) {
                            self.discovery(value).await?
                        } else {
                            value
                        };
                        reworded.insert(name, value);
                    }
                    Ok(Value::Object(reworded))
                }
                scalar @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => Ok(scalar),
            }
        })
    }

    /// Reword entity type names, one or an array of them.
    async fn type_names(&self, value: Value) -> Result<Value, FederationError> {
        match value {
            Value::String(name) => Ok(Value::String(self.type_name(&name).await?.to_string())),
            Value::Array(names) => {
                let mut reworded = Vec::with_capacity(names.len());
                for name in names {
                    reworded.push(match name {
                        Value::String(name) => Value::String(self.type_name(&name).await?.to_string()),
                        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => other,
                    });
                }
                Ok(Value::Array(reworded))
            }
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) => Ok(other),
        }
    }

    /// Reword attribute names, one or an array of them.
    async fn attribute_names(&self, value: Value) -> Result<Value, FederationError> {
        match value {
            Value::String(name) => Ok(Value::String(self.attribute_name(&name).await?.to_string())),
            Value::Array(names) => {
                let mut reworded = Vec::with_capacity(names.len());
                for name in names {
                    reworded.push(match name {
                        Value::String(name) => Value::String(self.attribute_name(&name).await?.to_string()),
                        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => other,
                    });
                }
                Ok(Value::Array(reworded))
            }
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) => Ok(other),
        }
    }

    /// One entity type name, from the spelling of the context the document is
    /// written in to the one it is read in.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the name does not expand
    /// against the written-in context, and
    /// [`FederationInvariant::ForwardedName`] when its compacted spelling is
    /// not an NGSI-LD name.
    async fn type_name(&self, name: &str) -> Result<EntityTypeName, FederationError> {
        let iri = self.engine.expand_type(name, &self.written_in).await?;
        reworded_name(&self.read_in.compact_type_iri(iri.as_str()))
    }

    /// One attribute name, from the spelling of the context the document is
    /// written in to the one it is read in.
    ///
    /// # Errors
    ///
    /// As [`DocumentRewording::type_name`].
    async fn attribute_name(&self, name: &str) -> Result<AttributeName, FederationError> {
        if is_core_term(name) {
            return reworded_name(name);
        }
        let iri = self.engine.expand_term(name, &self.written_in).await?;
        reworded_name(&self.read_in.compact_attr_iri(iri.as_str()))
    }
}

/// A reworded spelling as the NGSI-LD name it has to be: compaction answers a
/// term or a full IRI, both of which the clause 4.6.2 grammar admits, so a
/// refusal means the read-in context maps a name to text no name can be.
fn reworded_name<N>(spelling: &str) -> Result<N, FederationError>
where
    N: FromStr<Err = NgsiLdReferenceError>,
{
    spelling
        .parse()
        .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))
}

#[cfg(test)]
mod tests {
    use crate::{
        document_rewording::{DocumentRewording, DocumentVocabulary},
        forwarding_context::Rewording,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::{Value, json};

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The client's context.
    fn inbound() -> ContextSpec {
        ContextSpec::from_body(json!({
            "speed": "https://example.org/speed",
            "Vehicle": "https://example.org/Vehicle",
            "street": "https://example.org/street"
        }))
    }

    /// The registration's `jsonldContext`: the same IRIs under other terms.
    fn configured() -> ContextSpec {
        ContextSpec::from_body(json!({
            "velocity": "https://example.org/speed",
            "Car": "https://example.org/Vehicle",
            "road": "https://example.org/street"
        }))
    }

    async fn reword(written_in: &ContextSpec, read_in: &ContextSpec, document: Value, vocabulary: DocumentVocabulary) -> Value {
        DocumentRewording::reword(&engine(), written_in, read_in, document, vocabulary, Rewording::Reword)
            .await
            .expect("rewords")
    }

    #[tokio::test]
    async fn an_entity_answer_is_read_back_in_the_client_terms() {
        let answer = json!([{
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Car",
            "velocity": {
                "type": "Property",
                "value": 10,
                "observedAt": "2024-01-01T00:00:00Z",
                "velocity": {"type": "Property", "value": 1}
            },
            "address": {"type": "Property", "value": {"road": "Main"}},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}}
        }]);
        let reworded = reword(&configured(), &inbound(), answer, DocumentVocabulary::Entities).await;
        pretty_assertions::assert_eq!(
            reworded,
            json!([{
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "value": 10,
                    "observedAt": "2024-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 1}
                },
                "address": {"type": "Property", "value": {"street": "Main"}},
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}}
            }])
        );
    }

    #[tokio::test]
    async fn a_temporal_answer_rewords_every_instance() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Car",
            "velocity": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": 11, "observedAt": "2024-01-01T00:01:00Z"}
            ]
        });
        let reworded = reword(&configured(), &inbound(), answer, DocumentVocabulary::Entities).await;
        pretty_assertions::assert_eq!(reworded["type"], json!("Vehicle"));
        pretty_assertions::assert_eq!(reworded["speed"].as_array().map(Vec::len), Some(2));
    }

    /// A request body travels the other way: from the client's terms into the
    /// forwarding context's.
    #[tokio::test]
    async fn a_request_entity_is_spelled_in_the_forwarding_context() {
        let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}});
        let reworded = reword(&inbound(), &configured(), body, DocumentVocabulary::Entities).await;
        pretty_assertions::assert_eq!(
            reworded,
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Car", "velocity": {"type": "Property", "value": 10}})
        );
    }

    /// The attribute a request path names is reworded as an attribute: its
    /// core members stay, its sub-attributes and structured value members are
    /// spelled in the forwarding context.
    #[tokio::test]
    async fn an_attribute_body_rewords_what_it_names_and_keeps_its_core_members() {
        let body = json!({
            "type": "Property",
            "value": {"street": "Main"},
            "observedAt": "2024-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 1}
        });
        let reworded = reword(&inbound(), &configured(), body, DocumentVocabulary::Attribute).await;
        pretty_assertions::assert_eq!(
            reworded,
            json!({
                "type": "Property",
                "value": {"road": "Main"},
                "observedAt": "2024-01-01T00:00:00Z",
                "velocity": {"type": "Property", "value": 1}
            })
        );
    }

    #[tokio::test]
    async fn a_discovery_answer_rewords_its_type_and_attribute_lists() {
        let answer = json!({
            "id": "urn:ngsi-ld:EntityTypeList:1",
            "type": "EntityTypeList",
            "typeList": ["Car"],
            "attributeDetails": [{"id": "https://example.org/speed", "type": "Attribute", "attributeName": "velocity", "attributeTypes": ["Property"]}]
        });
        let reworded = reword(&configured(), &inbound(), answer, DocumentVocabulary::Discovery).await;
        pretty_assertions::assert_eq!(reworded["typeList"], json!(["Vehicle"]));
        pretty_assertions::assert_eq!(reworded["attributeDetails"][0]["attributeName"], json!("speed"));
        pretty_assertions::assert_eq!(reworded["attributeDetails"][0]["attributeTypes"], json!(["Property"]));
        pretty_assertions::assert_eq!(reworded["type"], json!("EntityTypeList"));
    }

    #[tokio::test]
    async fn a_document_in_the_client_context_is_left_alone() {
        let inbound = inbound();
        let answer = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Car", "velocity": {"type": "Property", "value": 1}});
        let kept = DocumentRewording::reword(
            &engine(),
            &inbound,
            &inbound,
            answer.clone(), // clone: compared below
            DocumentVocabulary::Entities,
            Rewording::AsInbound,
        )
        .await
        .expect("keeps");
        pretty_assertions::assert_eq!(kept, answer);
    }
}
