//! A rendered temporal entity read as a node of the Linked Entity engine.
//!
//! A temporal representation (clause 4.5.7) carries one instance per
//! observation of each Attribute, so an Attribute whose Relationship pointed
//! somewhere different at two instants names two Linked Entities. Every
//! instance of every Attribute is read: `object` (one URI or an array of them,
//! clause 4.5.3.2) and `objectList` in either spelling (clause 4.5.22), and an
//! `objectType` on any instance makes the edge eligible for a fetch from
//! outside the local store (clause 4.5.23.1). A single-instance Attribute may
//! have been normalised to a bare object, which names its targets the same
//! way. An object too long to be an [`Identifier`] names no entity any store
//! can hold, so it is no edge.
//!
//! The `temporalValues` and aggregated representations (clauses 4.5.8,
//! 4.5.19) carry no `object` member; the edges of a node rendered that way are
//! read off its normalized form before the projection, which the source hands
//! the engine beside the node.

use crate::relationship_targets::rendered_relationship_targets;
use andromeda_linked_retrieval::{
    edge::{LinkedEdge, RemoteEligibility},
    keyword::Keyword,
    representation::LinkedRepresentation,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use enumset::EnumSet;
use serde_json::{Map, Value};
use std::slice;

/// The member carrying the JSON-LD context of a rendered entity.
const JSON_LD_CONTEXT: &str = "@context";

/// The representation of the rendered temporal entities the temporal
/// operations join behind the `temporal_linked_retrieval` extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalEvolutions;

/// What one member of a rendered temporal entity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalMember {
    /// `id`, `type` or `scope`, which a clause 4.21 list keeps by keyword.
    Keyword(Keyword),
    /// The `@context` and the entity's clause 4.8 system times, which are no
    /// Attribute and which no list removes.
    Structural,
    /// An Attribute, named by its key.
    Attribute,
}

impl TemporalMember {
    /// The kind of the member keyed `key`.
    #[must_use]
    pub fn of(key: &str) -> TemporalMember {
        match Keyword::of_key(key) {
            Some(keyword) => TemporalMember::Keyword(keyword),
            None => match key {
                JSON_LD_CONTEXT | compact_properties::CREATED_AT | compact_properties::MODIFIED_AT | compact_properties::DELETED_AT => {
                    TemporalMember::Structural
                }
                // `&str` is unbounded; every other key names an Attribute.
                _ => TemporalMember::Attribute,
            },
        }
    }
}

impl LinkedRepresentation for TemporalEvolutions {
    type Node = Value;

    fn identifier(node: &Value) -> Option<Identifier> {
        node.get(ProjectionTerm::ID_KEYWORD).and_then(Value::as_str).and_then(|raw| raw.parse().ok())
    }

    fn edges(node: &Value, out: &mut Vec<LinkedEdge>) {
        let (Some(from), Some(members)) = (TemporalEvolutions::identifier(node), node.as_object()) else {
            return;
        };
        for (key, member) in attributes(members) {
            let Ok(attribute) = key.parse::<AttributeName>() else {
                continue;
            };
            for instance in instances_of(member).filter_map(Value::as_object) {
                instance_edges(&from, &attribute, instance, out);
            }
        }
    }

    fn attribute_keys(node: &Value) -> impl Iterator<Item = &str> {
        node.as_object().into_iter().flat_map(attributes).map(|(key, _)| key.as_str())
    }

    fn keyword_members(node: &Value) -> EnumSet<Keyword> {
        node.as_object()
            .into_iter()
            .flat_map(Map::keys)
            .filter_map(|key| Keyword::of_key(key))
            .collect()
    }
}

/// The Attribute members of a rendered temporal entity, in member order.
fn attributes(members: &Map<String, Value>) -> impl Iterator<Item = (&String, &Value)> {
    members.iter().filter(|(key, _)| TemporalMember::of(key) == TemporalMember::Attribute)
}

/// The instances one rendered Attribute member holds: an array of them, or
/// one normalised to a bare object.
fn instances_of(member: &Value) -> impl Iterator<Item = &Value> {
    match member {
        Value::Array(instances) => instances.iter(),
        single @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => slice::from_ref(single).iter(),
    }
}

/// The edges one rendered Attribute instance names, `object` before
/// `objectList`, each in order.
fn instance_edges(from: &Identifier, attribute: &AttributeName, instance: &Map<String, Value>, out: &mut Vec<LinkedEdge>) {
    let eligibility = if instance.contains_key(compact_properties::OBJECT_TYPE) {
        RemoteEligibility::Eligible
    } else {
        RemoteEligibility::LocalOnly
    };
    let objects = [compact_properties::OBJECT, compact_properties::OBJECT_LIST]
        .into_iter()
        .filter_map(|key| instance.get(key));
    for object in objects {
        out.extend(
            rendered_relationship_targets(object)
                .into_iter()
                .filter_map(|target| Identifier::try_from(target).ok())
                .map(|target| LinkedEdge {
                    from: from.clone(),           // clone: each edge owns the entity it leaves
                    attribute: attribute.clone(), // clone: each edge owns its attribute key
                    target,
                    eligibility,
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::temporal_evolutions::{TemporalEvolutions, TemporalMember};
    use andromeda_linked_retrieval::{
        edge::{LinkedEdge, RemoteEligibility},
        keyword::Keyword,
        representation::LinkedRepresentation,
    };
    use enumset::EnumSet;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn edges(node: &Value) -> Vec<(String, String, RemoteEligibility)> {
        let mut out: Vec<LinkedEdge> = Vec::new();
        TemporalEvolutions::edges(node, &mut out);
        out.into_iter()
            .map(|edge| (edge.attribute.to_string(), edge.target.to_string(), edge.eligibility))
            .collect()
    }

    fn building(members: Value) -> Value {
        let mut node = members;
        node["id"] = json!("urn:ngsi-ld:Building:A");
        node["type"] = json!("Building");
        node
    }

    fn local(attribute: &str, target: &str) -> (String, String, RemoteEligibility) {
        (attribute.to_owned(), target.to_owned(), RemoteEligibility::LocalOnly)
    }

    /// A temporal representation carries one Relationship instance per
    /// observation, and each names its own target, in instance order.
    #[test]
    fn an_instance_array_names_every_object() {
        let node = building(json!({"parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:C", "observedAt": "2024-01-02T00:00:00Z"}
        ]}));
        assert_eq!(edges(&node), vec![local("parkedAt", "urn:ngsi-ld:T:B"), local("parkedAt", "urn:ngsi-ld:T:C")]);
    }

    #[test]
    fn a_bare_instance_object_names_its_object() {
        let node = building(json!({"parkedAt": {"type": "Relationship", "object": "urn:ngsi-ld:T:B"}}));
        assert_eq!(edges(&node), vec![local("parkedAt", "urn:ngsi-ld:T:B")]);
    }

    /// Clause 4.5.22: both spellings of an `objectList` name their members in
    /// order.
    #[test]
    fn an_object_list_names_in_order() {
        let concise = building(json!({"parts": [{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]}]}));
        let normalized =
            building(json!({"parts": [{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:T:C"}, {"object": "urn:ngsi-ld:T:B"}]}]}));
        let expected = vec![local("parts", "urn:ngsi-ld:T:C"), local("parts", "urn:ngsi-ld:T:B")];
        assert_eq!(edges(&concise), expected);
        assert_eq!(edges(&normalized), expected);
    }

    /// Clause 4.5.23.1: an `objectType` on an instance makes its targets
    /// Internal Linked Entities.
    #[test]
    fn an_object_type_on_any_instance_makes_the_edge_eligible() {
        let node = building(json!({"parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "objectType": "Thing"}
        ]}));
        let eligibility: Vec<RemoteEligibility> = edges(&node).into_iter().map(|(_, _, eligibility)| eligibility).collect();
        assert_eq!(eligibility, vec![RemoteEligibility::LocalOnly, RemoteEligibility::Eligible]);
    }

    /// The structural members are no Attribute, and a Property naming a URI is
    /// no Relationship.
    #[test]
    fn structural_members_are_skipped() {
        let node = building(json!({
            "createdAt": "urn:ngsi-ld:T:B",
            "@context": "urn:ngsi-ld:T:B",
            "name": [{"type": "Property", "value": "urn:ngsi-ld:T:B"}]
        }));
        assert_eq!(edges(&node), Vec::new());
        assert_eq!(TemporalEvolutions::attribute_keys(&node).collect::<Vec<_>>(), vec!["name"]);
    }

    #[test]
    fn an_object_too_long_for_an_identifier_is_no_edge() {
        let long = format!("urn:ngsi-ld:T:{}", "b".repeat(1_100));
        let node = building(json!({"parkedAt": [{"type": "Relationship", "object": long}]}));
        assert_eq!(edges(&node), Vec::new());
    }

    #[test]
    fn the_keyword_members_are_the_ones_the_node_carries() {
        let unscoped = building(json!({}));
        let scoped = building(json!({"scope": "/Madrid"}));
        assert_eq!(TemporalEvolutions::keyword_members(&unscoped), Keyword::Id | Keyword::Type);
        assert_eq!(TemporalEvolutions::keyword_members(&scoped), EnumSet::all());
    }

    #[test]
    fn every_member_key_has_one_kind() {
        assert_eq!(TemporalMember::of("id"), TemporalMember::Keyword(Keyword::Id));
        assert_eq!(TemporalMember::of("scope"), TemporalMember::Keyword(Keyword::Scope));
        assert_eq!(TemporalMember::of("modifiedAt"), TemporalMember::Structural);
        assert_eq!(TemporalMember::of("@context"), TemporalMember::Structural);
        assert_eq!(TemporalMember::of("speed"), TemporalMember::Attribute);
    }
}
