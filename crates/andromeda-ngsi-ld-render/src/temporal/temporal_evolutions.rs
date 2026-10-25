//! A Temporal Evolution of an entity read as a node of the Linked Entity
//! engine.
//!
//! A temporal representation (clause 4.5.7) carries one instance per
//! observation of each Attribute, so an Attribute whose Relationship pointed
//! somewhere different at two instants names two Linked Entities. Every
//! instance of every Attribute is read: `object` (one URI or an array of them,
//! clause 4.5.3.2) and `objectList` (clause 4.5.22), and an `objectType` on any
//! instance makes the edge eligible for a fetch from outside the local store
//! (clause 4.5.23.1). An object too long to be an identifier names no entity any
//! store can hold, so it is no edge.
//!
//! The aggregated representation (clause 4.5.19) carries no instance, so an
//! evolution in it names no edge; the edges of a root rendered that way are the
//! ones its normalized form named before the aggregation, which the source
//! hands the engine beside the node.

use andromeda_linked_retrieval::{edge::LinkedEdge, keyword::Keyword, representation::LinkedRepresentation, typed::instance_edges};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::temporal_body::TemporalBody;
use enumset::EnumSet;

/// The representation of the Temporal Evolutions the temporal operations join
/// behind the `temporal_linked_retrieval` extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalEvolutions;

impl LinkedRepresentation for TemporalEvolutions {
    type Node = TemporalBody;

    fn identifier(node: &TemporalBody) -> Option<Identifier> {
        Some(node.id().clone()) // clone: the traversal keys its nodes by owned identifiers
    }

    fn edges(node: &TemporalBody, out: &mut Vec<LinkedEdge>) {
        match node {
            TemporalBody::Evolution(entity) => {
                for (attribute, evolution) in &entity.attrs {
                    for instance in evolution.instances() {
                        instance_edges(&entity.id, attribute, instance, out);
                    }
                }
            }
            TemporalBody::Aggregated(_) => {}
        }
    }

    fn attribute_keys(node: &TemporalBody) -> impl Iterator<Item = &str> {
        let (evolution, aggregated) = match node {
            TemporalBody::Evolution(entity) => (Some(entity), None),
            TemporalBody::Aggregated(entity) => (None, Some(entity)),
        };
        evolution
            .into_iter()
            .flat_map(|entity| entity.attrs.keys())
            .chain(aggregated.into_iter().flat_map(|entity| entity.attrs.keys()))
            .map(AttributeName::as_str)
    }

    fn keyword_members(node: &TemporalBody) -> EnumSet<Keyword> {
        match node {
            TemporalBody::Evolution(entity) if entity.scope.is_some() => Keyword::Id | Keyword::Type | Keyword::Scope,
            TemporalBody::Evolution(_) | TemporalBody::Aggregated(_) => Keyword::Id | Keyword::Type,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::temporal_evolutions::TemporalEvolutions;
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_linked_retrieval::{
        edge::{LinkedEdge, RemoteEligibility},
        keyword::Keyword,
        representation::LinkedRepresentation,
    };
    use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_entity::TemporalEntity};
    use enumset::EnumSet;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json, value::RawValue};

    fn evolution(members: Value) -> TemporalBody {
        let mut document = members;
        document["id"] = json!("urn:ngsi-ld:Building:A");
        document["type"] = json!("Building");
        let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
        TemporalBody::Evolution(TemporalEntity::from_node(JsonPiece::new(&raw)).expect("an evolution"))
    }

    fn edges(node: &TemporalBody) -> Vec<(String, String, RemoteEligibility)> {
        let mut out: Vec<LinkedEdge> = Vec::new();
        TemporalEvolutions::edges(node, &mut out);
        out.into_iter()
            .map(|edge| (edge.attribute.to_string(), edge.target.to_string(), edge.eligibility))
            .collect()
    }

    fn local(attribute: &str, target: &str) -> (String, String, RemoteEligibility) {
        (attribute.to_owned(), target.to_owned(), RemoteEligibility::LocalOnly)
    }

    /// A temporal representation carries one Relationship instance per
    /// observation, and each names its own target, in instance order.
    #[test]
    fn an_instance_array_names_every_object() {
        let node = evolution(json!({"parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:C", "observedAt": "2024-01-02T00:00:00Z"}
        ]}));
        assert_eq!(edges(&node), vec![local("parkedAt", "urn:ngsi-ld:T:B"), local("parkedAt", "urn:ngsi-ld:T:C")]);
    }

    /// Clause 4.5.22: an `objectList` names its members in order.
    #[test]
    fn an_object_list_names_in_order() {
        let node = evolution(json!({"parts": [{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]}]}));
        assert_eq!(edges(&node), vec![local("parts", "urn:ngsi-ld:T:C"), local("parts", "urn:ngsi-ld:T:B")]);
    }

    /// Clause 4.5.23.1: an `objectType` on an instance makes its targets
    /// Internal Linked Entities.
    #[test]
    fn an_object_type_on_any_instance_makes_the_edge_eligible() {
        let node = evolution(json!({"parkedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B"},
            {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "objectType": "Thing"}
        ]}));
        let eligibility: Vec<RemoteEligibility> = edges(&node).into_iter().map(|(_, _, eligibility)| eligibility).collect();
        assert_eq!(eligibility, vec![RemoteEligibility::LocalOnly, RemoteEligibility::Eligible]);
    }

    /// A Property naming a URI is no Relationship, and the structural members
    /// are no Attribute.
    #[test]
    fn properties_and_structural_members_name_nothing() {
        let node = evolution(json!({
            "createdAt": "2024-01-01T00:00:00Z",
            "name": [{"type": "Property", "value": "urn:ngsi-ld:T:B"}]
        }));
        assert_eq!(edges(&node), Vec::new());
        assert_eq!(TemporalEvolutions::attribute_keys(&node).collect::<Vec<_>>(), vec!["name"]);
    }

    #[test]
    fn an_object_too_long_for_an_identifier_is_no_edge() {
        let long = format!("urn:ngsi-ld:T:{}", "b".repeat(1_100));
        let node = evolution(json!({"parkedAt": [{"type": "Relationship", "object": long}]}));
        assert_eq!(edges(&node), Vec::new());
    }

    #[test]
    fn the_keyword_members_are_the_ones_the_node_carries() {
        let unscoped = evolution(json!({}));
        let scoped = evolution(json!({"scope": [{"type": "Property", "value": "/Madrid", "observedAt": "2024-01-01T00:00:00Z"}]}));
        assert_eq!(TemporalEvolutions::keyword_members(&unscoped), Keyword::Id | Keyword::Type);
        assert_eq!(TemporalEvolutions::keyword_members(&scoped), EnumSet::all());
    }
}
