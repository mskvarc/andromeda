//! The steps a filter's `rel{…}` hops take out of an entity (clause 4.9).
//!
//! An in-memory evaluation of a hop reads the linked entities from a source
//! its caller fills beforehand. What that source has to hold follows from the
//! filter and the entity alone: each hop crosses the objects of one
//! Relationship, and continues along the rest of its path in the entity each
//! object names. Listing those steps here keeps the gathering and the
//! evaluation on one reading of which objects a hop crosses.

use crate::{
    filter::{attribute_axis::relationship_objects, expression::Expr, path::QueryPath},
    queried_entity::attribute_reading::QueriedEntity,
};

/// One Relationship object a hop crosses, with the path the hop continues
/// along in the entity it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopStep<'a, 'q> {
    /// The object: the identifier of the linked entity.
    pub object: &'a str,
    /// The path left after the hop, evaluated against the linked entity.
    pub inner: &'q QueryPath,
}

/// The attribute axes of an expression's Query Terms that cross into a linked
/// entity, in pre-order.
#[must_use]
pub fn linked_paths(expr: &Expr) -> Vec<&QueryPath> {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter().flat_map(linked_paths).collect(),
        Expr::Pred(pred) => {
            let path = pred.path();
            if path.is_linked() { vec![path] } else { Vec::new() }
        }
    }
}

/// Every step the hops of `paths` take out of `entity`: one per object of each
/// hop's Relationship, the same objects the evaluator crosses. A wholly local
/// path takes no step.
pub fn hop_steps<'a, 'q, E: QueriedEntity<'a>>(entity: E, paths: &[&'q QueryPath]) -> Vec<HopStep<'a, 'q>> {
    paths
        .iter()
        .flat_map(|path| match path {
            QueryPath::Linked { relationship, inner, .. } => relationship_objects(entity, relationship)
                .map(|object| HopStep { object, inner: inner.as_ref() })
                .collect(),
            QueryPath::Local(_) => Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            hop_steps::{hop_steps, linked_paths},
            parser::parse,
            path::QueryPath,
        },
        queried_entity::expanded_fixture::FixtureEntity,
    };
    use serde_json::json;

    /// An entity whose `a` Relationship names X through one instance and Y
    /// through another, and whose `p` Property spells a URI.
    fn entity() -> FixtureEntity {
        FixtureEntity::from_expanded(&json!({
            "@id": "urn:root",
            "@type": ["https://example.com/T"],
            "https://example.com/a": [
                {"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:x"}]},
                {
                    "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:y"}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:other"}]
                }
            ],
            "https://example.com/p": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:z"}]}]
        }))
    }

    #[test]
    fn hop_steps_pair_each_object_with_the_rest_of_its_path() {
        let expr = parse("\"https://example.com/a\"{\"https://example.com/b\"{\"https://example.com/c\"==1}}").expect("parses");
        let paths = linked_paths(&expr);
        let entity = entity();
        let steps = hop_steps(&entity, &paths);
        let objects: Vec<&str> = steps.iter().map(|step| step.object).collect();
        pretty_assertions::assert_eq!(objects, vec!["urn:x", "urn:y"]);
        let QueryPath::Linked { relationship, inner, .. } = steps[0].inner else {
            panic!("the rest of the path is the second hop");
        };
        pretty_assertions::assert_eq!(relationship.as_str(), "https://example.com/b");
        assert!(!inner.is_linked());
    }

    #[test]
    fn an_expression_without_a_hop_takes_no_step() {
        let expr = parse("\"https://example.com/a\"==\"urn:x\";\"https://example.com/p\"").expect("parses");
        pretty_assertions::assert_eq!(linked_paths(&expr), Vec::<&QueryPath>::new());
        let through_property = parse("\"https://example.com/p\"{\"https://example.com/c\"}").expect("parses");
        let entity = entity();
        pretty_assertions::assert_eq!(
            hop_steps(&entity, &linked_paths(&through_property)),
            Vec::new(),
            "a Property names no linked entity"
        );
    }
}
