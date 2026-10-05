//! Writing the Linked Entities of a joined temporal payload (clause 4.5.23)
//! the way the Linked Entity engine planned them.
//!
//! A flat join (clause 4.5.23.3) appends each planned Linked Entity after the
//! roots, narrowed by the list its first encounter decided; an inline join
//! (clause 4.5.23.2) nests copies under the roots' Relationship instances.
//! Every decision is the plan's; this module only spells it over rendered
//! temporal entities.

use crate::temporal::{inline_join::write_temporal_inline_join, linked_projection::narrowed_temporal_entity, temporal_evolutions::TemporalEvolutions};
use andromeda_linked_retrieval::plan::JoinPlan;
use serde_json::Value;

/// Where a join put the Linked Entities of a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinedShape {
    /// Appended after the roots, so the payload is a collection.
    Appended,
    /// Nested inside the roots, which are the whole payload.
    Nested,
}

/// Write `plan` into the rendered `roots`, and say where it put the Linked
/// Entities.
pub fn write_temporal_join(plan: &JoinPlan<'_, TemporalEvolutions>, roots: &mut Vec<Value>) -> JoinedShape {
    match plan {
        JoinPlan::Flat(flat) => {
            roots.extend(flat.linked().map(|linked| narrowed_temporal_entity(linked.node, linked.rule)));
            JoinedShape::Appended
        }
        JoinPlan::Inline(inline) => {
            for root in roots.iter_mut() {
                write_temporal_inline_join(root, inline.cursor());
            }
            JoinedShape::Nested
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::{
        join_writer::{JoinedShape, write_temporal_join},
        temporal_evolutions::TemporalEvolutions,
    };
    use andromeda_linked_retrieval::{
        graph::{LinkedGraph, RootEdges},
        join::RetrievingJoin,
        narrowing::{list::ListInForce, list_terms::ListTerms, member::MemberRule},
        plan::JoinPlan,
        representation::LinkedRepresentation,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::num::NonZeroU32;

    fn root() -> Value {
        json!({
            "id": "urn:ngsi-ld:B:1",
            "type": "Building",
            "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
            "locatedIn": [{"type": "Relationship", "object": "urn:ngsi-ld:C:1"}]
        })
    }

    fn nodes() -> Vec<Value> {
        vec![
            json!({"id": "urn:ngsi-ld:P:1", "type": "Person", "name": [{"type": "Property", "value": "Ada"}], "email": [{"type": "Property", "value": "a@b.c"}]}),
            json!({"id": "urn:ngsi-ld:C:1", "type": "City", "name": [{"type": "Property", "value": "Ljubljana"}]}),
        ]
    }

    fn graph(root: &Value, nodes: Vec<Value>) -> LinkedGraph<Value> {
        let RootEdges { ids, mut edges } = RootEdges::of::<TemporalEvolutions>([root]);
        for node in &nodes {
            TemporalEvolutions::edges(node, &mut edges);
        }
        LinkedGraph {
            roots: ids,
            nodes: nodes
                .into_iter()
                .map(|node| (TemporalEvolutions::identifier(&node).expect("an identifier"), node))
                .collect(),
            edges,
            failures: Vec::new(),
            depth: NonZeroU32::MIN,
        }
    }

    fn index() -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in ["owner", "locatedIn", "name", "email"] {
            index.insert(
                key.parse::<AttributeName>().expect("valid attribute name"),
                IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI"),
            );
        }
        index
    }

    /// Clause 4.5.23.3: a flat join appends the Linked Entities after the
    /// roots, in first-encounter order, each narrowed by its list (D3).
    #[test]
    fn a_flat_join_appends_the_narrowed_linked_entities_after_the_roots() {
        let index = index();
        let graph = graph(&root(), nodes());
        let pick = [
            ProjectionTerm::Id,
            ProjectionTerm::Attribute {
                iri: IriBuf::new("https://example.com/owner".to_owned()).expect("valid IRI"),
                linked: Vec::new(),
            },
            ProjectionTerm::Attribute {
                iri: IriBuf::new("https://example.com/name".to_owned()).expect("valid IRI"),
                linked: Vec::new(),
            },
        ];
        let plan = JoinPlan::<TemporalEvolutions>::new(
            RetrievingJoin::Flat,
            &graph,
            MemberRule::new(ListInForce::Payload(ListTerms::Pick(&pick)), &index),
        );
        let mut roots = vec![root()];

        assert_eq!(write_temporal_join(&plan, &mut roots), JoinedShape::Appended);
        assert_eq!(roots.len(), 2, "locatedIn is not returned, so C:1 is not reached");
        assert_eq!(roots[1], json!({"id": "urn:ngsi-ld:P:1", "name": [{"type": "Property", "value": "Ada"}]}));
    }

    /// Clause 4.5.23.2: an inline join nests rather than appends.
    #[test]
    fn an_inline_join_nests_without_appending() {
        let index = index();
        let graph = graph(&root(), nodes());
        let plan = JoinPlan::<TemporalEvolutions>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
        let mut roots = vec![root()];

        assert_eq!(write_temporal_join(&plan, &mut roots), JoinedShape::Nested);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0]["owner"][0]["entity"]["id"], "urn:ngsi-ld:P:1");
        assert_eq!(roots[0]["locatedIn"][0]["entity"]["id"], "urn:ngsi-ld:C:1");
    }
}
