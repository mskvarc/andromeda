//! The typed [`Entity`] read as a node of the engine.
//!
//! Clause 4.5.23.1 joins through Relationships and `ListRelationships`: a
//! Relationship names its `object` (one URI or an ordered array of them), a
//! `ListRelationship` its `objectList` in order, and every dataset instance of
//! an Attribute names its own. An `objectType` on the instance makes the edge
//! eligible for a fetch from outside the local store. An object too long to be
//! an [`Identifier`] names no entity any store can hold, so it is no edge.

use crate::{
    edge::{LinkedEdge, RemoteEligibility},
    keyword::Keyword,
    representation::LinkedRepresentation,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_model::{attribute::instance::Attribute, entity::Entity};
use enumset::EnumSet;

/// The representation of the typed [`Entity`] every Core read and both kinds
/// of notification join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedEntities;

impl LinkedRepresentation for TypedEntities {
    type Node = Entity;

    fn identifier(node: &Entity) -> Option<Identifier> {
        Some(node.id.clone()) // clone: the traversal keys its nodes by owned identifiers
    }

    fn edges(node: &Entity, out: &mut Vec<LinkedEdge>) {
        for (attribute, value) in &node.attrs {
            for instance in value.instances() {
                instance_edges(&node.id, attribute, instance, out);
            }
        }
    }

    fn attribute_keys(node: &Entity) -> impl Iterator<Item = &str> {
        node.attribute_member_names()
    }

    fn keyword_members(node: &Entity) -> EnumSet<Keyword> {
        match node.scope {
            Some(_) => Keyword::Id | Keyword::Type | Keyword::Scope,
            None => Keyword::Id | Keyword::Type,
        }
    }
}

/// The edges one attribute instance names, appended to `out`: the targets of a
/// Relationship's `object` or of a `ListRelationship`'s `objectList`, in order.
/// Every representation of an entity that holds typed instances reads its
/// edges through this.
pub fn instance_edges(from: &Identifier, attribute: &AttributeName, instance: &Attribute, out: &mut Vec<LinkedEdge>) {
    match instance {
        Attribute::Relationship(relationship) => {
            targets_of(from, attribute, relationship.object.iter(), eligibility(relationship.object_type.as_ref()), out);
        }
        Attribute::ListRelationship(list) => {
            targets_of(from, attribute, list.object_list.iter(), eligibility(list.object_type.as_ref()), out);
        }
        Attribute::Property(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::VocabProperty(_) => {}
    }
}

/// Whether an instance's `objectType` annotation makes its targets eligible.
const fn eligibility<T>(object_type: Option<&T>) -> RemoteEligibility {
    match object_type {
        Some(_) => RemoteEligibility::Eligible,
        None => RemoteEligibility::LocalOnly,
    }
}

/// One edge per object an identifier can hold.
fn targets_of<'a>(
    from: &Identifier,
    attribute: &AttributeName,
    objects: impl Iterator<Item = &'a Uri>,
    eligibility: RemoteEligibility,
    out: &mut Vec<LinkedEdge>,
) {
    // clone: each edge owns its target, and an object too long for an
    // identifier is skipped
    out.extend(objects.filter_map(|object| Identifier::try_from(object.clone()).ok()).map(|target| LinkedEdge {
        from: from.clone(),           // clone: each edge owns the entity it leaves
        attribute: attribute.clone(), // clone: each edge owns its attribute key
        target,
        eligibility,
    }));
}

#[cfg(test)]
mod tests {
    use crate::{
        edge::{LinkedEdge, RemoteEligibility},
        keyword::Keyword,
        representation::LinkedRepresentation,
        typed::TypedEntities,
    };
    use andromeda_ngsi_ld_model::entity::Entity;
    use enumset::EnumSet;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn entity(document: Value) -> Entity {
        serde_json::from_value(document).expect("entity")
    }

    fn building(attributes: Value) -> Entity {
        let mut document = attributes;
        document["id"] = json!("urn:ngsi-ld:Building:A");
        document["type"] = json!("Building");
        entity(document)
    }

    fn edges(entity: &Entity) -> Vec<LinkedEdge> {
        let mut out = Vec::new();
        TypedEntities::edges(entity, &mut out);
        out
    }

    fn summary(edges: &[LinkedEdge]) -> Vec<(&str, &str, &str, RemoteEligibility)> {
        edges
            .iter()
            .map(|edge| (edge.from.as_str(), edge.attribute.as_str(), edge.target.as_str(), edge.eligibility))
            .collect()
    }

    #[test]
    fn a_relationship_names_its_object() {
        let entity = building(json!({"owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:P"}}));
        assert_eq!(
            summary(&edges(&entity)),
            vec![("urn:ngsi-ld:Building:A", "owner", "urn:ngsi-ld:Person:P", RemoteEligibility::LocalOnly)]
        );
    }

    #[test]
    fn an_object_array_names_every_target_in_order() {
        let entity = building(json!({"owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:P", "urn:ngsi-ld:Person:Q"]}}));
        let targets: Vec<String> = edges(&entity).into_iter().map(|edge| edge.target.to_string()).collect();
        assert_eq!(targets, vec!["urn:ngsi-ld:Person:P", "urn:ngsi-ld:Person:Q"]);
    }

    #[test]
    fn a_list_relationship_names_its_object_list_in_order() {
        let entity = building(json!({"owners": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:P", "urn:ngsi-ld:Person:Q"]}}));
        let targets: Vec<String> = edges(&entity).into_iter().map(|edge| edge.target.to_string()).collect();
        assert_eq!(targets, vec!["urn:ngsi-ld:Person:P", "urn:ngsi-ld:Person:Q"]);
    }

    /// Clause 4.5.23.1: an `objectType` declares the object an Internal
    /// Linked Entity.
    #[test]
    fn an_object_type_makes_the_edge_eligible() {
        let entity = building(json!({"owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:P", "objectType": "Person"}}));
        assert_eq!(edges(&entity)[0].eligibility, RemoteEligibility::Eligible);
    }

    #[test]
    fn properties_name_nothing() {
        let entity = building(json!({
            "name": {"type": "Property", "value": "urn:ngsi-ld:Person:P"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
            "geo": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
            "data": {"type": "JsonProperty", "json": {"object": "urn:ngsi-ld:Person:P"}},
            "label": {"type": "LanguageProperty", "languageMap": {"en": "a"}},
            "list": {"type": "ListProperty", "valueList": ["urn:ngsi-ld:Person:P"]},
            "kind": {"type": "VocabProperty", "vocab": "Person"}
        }));
        assert_eq!(edges(&entity), Vec::new());
    }

    #[test]
    fn an_object_too_long_for_an_identifier_is_no_edge() {
        let long = format!("urn:ngsi-ld:Person:{}", "p".repeat(1_100));
        let entity = building(json!({"owner": {"type": "Relationship", "object": long}}));
        assert_eq!(edges(&entity), Vec::new());
    }

    #[test]
    fn every_dataset_instance_contributes_its_edges() {
        let entity = building(json!({"owner": [
            {"type": "Relationship", "object": "urn:ngsi-ld:Person:P"},
            {"type": "Relationship", "object": "urn:ngsi-ld:Person:Q", "datasetId": "urn:ngsi-ld:Dataset:second"}
        ]}));
        let targets: Vec<String> = edges(&entity).into_iter().map(|edge| edge.target.to_string()).collect();
        assert_eq!(targets, vec!["urn:ngsi-ld:Person:P", "urn:ngsi-ld:Person:Q"]);
    }

    #[test]
    fn the_keyword_members_follow_the_scope() {
        let unscoped = building(json!({}));
        let scoped = entity(json!({"id": "urn:ngsi-ld:Building:A", "type": "Building", "scope": "/Madrid"}));
        assert_eq!(TypedEntities::keyword_members(&unscoped), Keyword::Id | Keyword::Type);
        assert_eq!(TypedEntities::keyword_members(&scoped), EnumSet::all());
    }

    #[test]
    fn the_attribute_keys_include_the_geo_properties() {
        let entity = building(json!({
            "name": {"type": "Property", "value": "A"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }));
        let mut keys: Vec<&str> = TypedEntities::attribute_keys(&entity).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["location", "name"]);
    }
}
