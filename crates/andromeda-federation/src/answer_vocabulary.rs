//! Reading a source's answer back in the client's terms (clause 4.3.6.6).
//!
//! A source answers a forwarded request in the context the request named
//! ([`crate::forwarding_context`]). When that is the inbound request's own
//! context the answer is already in the client's terms. When it is another one
//! -- the registration's `jsonldContext`, or the core context standing in for a
//! compound inbound context -- every name in the answer is spelled in that
//! context, and the merged result would mix two vocabularies. So each name is
//! expanded against the forwarding context and compacted against the inbound
//! one, while the answer's shape, identifiers and values stay as the source
//! sent them.
//!
//! What counts as a name depends on the kind of answer: an entity (or a
//! temporal entity) names its type and its attributes, and a discovery answer
//! (clauses 5.7.5 to 5.7.10) names entity types and attributes in its list
//! members. Core members keep their spelling in every context, because the core
//! context has absolute precedence (clause 5.5.7).

use crate::{error::FederationError, federation_invariant::FederationInvariant, forwarding_context::Rewording};
use andromeda_jsonld::{
    compaction::NameCompactor,
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, compact_to_expanded};
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin, str::FromStr};

/// The members of a discovery answer that hold entity type names.
const TYPE_NAME_MEMBERS: [&str; 3] = ["typeList", "typeNames", "typeName"];
/// The members of a discovery answer that hold attribute names.
const ATTRIBUTE_NAME_MEMBERS: [&str; 3] = ["attributeList", "attributeNames", "attributeName"];
/// The members of a discovery answer that hold nested discovery objects.
const NESTED_DISCOVERY_MEMBERS: [&str; 1] = ["attributeDetails"];
/// The core context's aliases of the JSON-LD `@id` and `@type` keywords.
const CORE_KEYWORD_ALIASES: [&str; 2] = ["id", "type"];
/// The entity members that are not attributes.
const ENTITY_IDENTITY_MEMBERS: [&str; 3] = ["id", "type", "@context"];

/// What kind of answer a forwarded read returns, which decides where its names
/// sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnswerVocabulary {
    /// An entity, a temporal entity, or an array of them (clauses 5.7.1 to
    /// 5.7.4).
    Entities,
    /// A type or attribute discovery answer (clauses 5.7.5 to 5.7.10).
    Discovery,
    /// An answer that carries no names, such as an `EntityMap` (clause 5.14).
    Opaque,
}

/// Rewords a source's answer from the forwarding context to the inbound one.
pub(crate) struct AnswerRewording<'a> {
    /// The engine that expands.
    engine: &'a JsonLdEngine,
    /// The forwarding context, prepared for term lookups.
    forwarding: PreparedTermContext,
    /// Compacts names against the inbound context.
    inbound: NameCompactor<'a>,
}

impl<'a> AnswerRewording<'a> {
    /// A rewording of answers written in `forwarding` into `inbound`'s terms.
    pub(crate) fn new(engine: &'a JsonLdEngine, forwarding: &ContextSpec, inbound: &'a ContextSpec) -> AnswerRewording<'a> {
        AnswerRewording {
            engine,
            forwarding: engine.prepare_term_context(forwarding),
            inbound: NameCompactor::new(engine, inbound),
        }
    }

    /// Reword one answer of `vocabulary`, or return it unchanged when the
    /// forwarding context is the inbound one.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a name in the answer does not
    /// expand against the forwarding context.
    pub(crate) async fn reword(&self, answer: Value, vocabulary: AnswerVocabulary, rewording: Rewording) -> Result<Value, FederationError> {
        match (rewording, vocabulary) {
            (Rewording::AsInbound, _) | (Rewording::Reword, AnswerVocabulary::Opaque) => Ok(answer),
            (Rewording::Reword, AnswerVocabulary::Entities) => match answer {
                Value::Array(entities) => {
                    let mut reworded = Vec::with_capacity(entities.len());
                    for entity in entities {
                        reworded.push(self.entity(entity).await?);
                    }
                    Ok(Value::Array(reworded))
                }
                entity @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => self.entity(entity).await,
            },
            (Rewording::Reword, AnswerVocabulary::Discovery) => self.discovery(answer).await,
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
            return self.reword(value, AnswerVocabulary::Entities, Rewording::Reword).await;
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

    /// One entity type name, from the forwarding context's spelling to the
    /// inbound one's.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the name does not expand
    /// against the forwarding context, and [`FederationInvariant::ForwardedName`]
    /// when its compacted spelling is not an NGSI-LD name.
    async fn type_name(&self, name: &str) -> Result<EntityTypeName, FederationError> {
        let iri = self.engine.expand_type(name, &self.forwarding).await?;
        read_back_name(&self.inbound.compact_type_iri(iri.as_str()).await)
    }

    /// One attribute name, from the forwarding context's spelling to the
    /// inbound one's.
    ///
    /// # Errors
    ///
    /// As [`AnswerRewording::type_name`].
    async fn attribute_name(&self, name: &str) -> Result<AttributeName, FederationError> {
        if is_core_term(name) {
            return read_back_name(name);
        }
        let iri = self.engine.expand_term(name, &self.forwarding).await?;
        read_back_name(&self.inbound.compact_attr_iri(iri.as_str()).await)
    }
}

/// A read-back spelling as the NGSI-LD name it has to be: compaction answers a
/// term or a full IRI, both of which the clause 4.6.2 grammar admits, so a
/// refusal means the inbound context maps a name to text no name can be.
fn read_back_name<N>(spelling: &str) -> Result<N, FederationError>
where
    N: FromStr<Err = NgsiLdReferenceError>,
{
    spelling
        .parse()
        .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))
}

/// Whether `name` is a JSON-LD keyword or a term of the core context, spelled
/// the same in every context because the core context has absolute precedence
/// (clause 5.5.7). The core context's keyword aliases `id` and `type` name no
/// IRI, so the term table does not hold them.
fn is_core_term(name: &str) -> bool {
    name.starts_with('@') || CORE_KEYWORD_ALIASES.contains(&name) || compact_to_expanded(name).is_some()
}

#[cfg(test)]
mod tests {
    use crate::{
        answer_vocabulary::{AnswerRewording, AnswerVocabulary},
        forwarding_context::Rewording,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::json;

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

    #[tokio::test]
    async fn an_entity_answer_is_read_back_in_the_client_terms() {
        let engine = engine();
        let (inbound, forwarding) = (inbound(), configured());
        let rewording = AnswerRewording::new(&engine, &forwarding, &inbound);
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
        let reworded = rewording.reword(answer, AnswerVocabulary::Entities, Rewording::Reword).await.expect("rewords");
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
        let engine = engine();
        let (inbound, forwarding) = (inbound(), configured());
        let rewording = AnswerRewording::new(&engine, &forwarding, &inbound);
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Car",
            "velocity": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": 11, "observedAt": "2024-01-01T00:01:00Z"}
            ]
        });
        let reworded = rewording.reword(answer, AnswerVocabulary::Entities, Rewording::Reword).await.expect("rewords");
        pretty_assertions::assert_eq!(reworded["type"], json!("Vehicle"));
        pretty_assertions::assert_eq!(reworded["speed"].as_array().map(Vec::len), Some(2));
    }

    #[tokio::test]
    async fn a_discovery_answer_rewords_its_type_and_attribute_lists() {
        let engine = engine();
        let (inbound, forwarding) = (inbound(), configured());
        let rewording = AnswerRewording::new(&engine, &forwarding, &inbound);
        let answer = json!({
            "id": "urn:ngsi-ld:EntityTypeList:1",
            "type": "EntityTypeList",
            "typeList": ["Car"],
            "attributeDetails": [{"id": "https://example.org/speed", "type": "Attribute", "attributeName": "velocity", "attributeTypes": ["Property"]}]
        });
        let reworded = rewording.reword(answer, AnswerVocabulary::Discovery, Rewording::Reword).await.expect("rewords");
        pretty_assertions::assert_eq!(reworded["typeList"], json!(["Vehicle"]));
        pretty_assertions::assert_eq!(reworded["attributeDetails"][0]["attributeName"], json!("speed"));
        pretty_assertions::assert_eq!(reworded["attributeDetails"][0]["attributeTypes"], json!(["Property"]));
        pretty_assertions::assert_eq!(reworded["type"], json!("EntityTypeList"));
    }

    #[tokio::test]
    async fn an_answer_in_the_client_context_is_left_alone() {
        let engine = engine();
        let inbound = inbound();
        let rewording = AnswerRewording::new(&engine, &inbound, &inbound);
        let answer = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Car", "velocity": {"type": "Property", "value": 1}});
        let kept = rewording
            .reword(answer.clone(), AnswerVocabulary::Entities, Rewording::AsInbound) // clone: compared below
            .await
            .expect("keeps");
        pretty_assertions::assert_eq!(kept, answer);
    }
}
