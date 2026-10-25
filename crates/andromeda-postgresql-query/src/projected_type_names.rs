//! Which entity type names a compiled query has to resolve against the
//! `entity_type_names` dictionary before its SQL can be rendered.
//!
//! Two parts of a query test entity types by dictionary key: the type
//! selection of every entity selector (clause 4.17) and the type guard of a
//! linked-entity traversal inside a `q` filter (clause 4.9). Both are
//! collected here so the read path resolves them in one lookup.

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::{expression::Expr, path::QueryPath},
    type_selection::grammar::TypeExpr,
};

/// Collect the type names a query tests by dictionary key.
///
/// The type selection carries expanded IRIs and a traversal guard carries
/// name tokens; both are the text the dictionary stores, and both satisfy the
/// clause 4.6.2 grammar an [`EntityTypeName`] validates, so a token that
/// nevertheless fails to parse is left out and its predicate later compiles as
/// an unresolved — and therefore unsatisfiable — type.
#[must_use]
pub fn projected_type_names(selection: &EntitySelection) -> Vec<EntityTypeName> {
    let mut names = Vec::new();
    for selector in &selection.selectors {
        if let Some(type_expr) = &selector.type_expr {
            collect_selection(type_expr, &mut names);
        }
    }
    if let Some(q_expr) = &selection.q_expr {
        collect_guards(q_expr, &mut names);
    }
    names
}

/// Every type IRI a type selection mentions.
fn collect_selection(expr: &TypeExpr, out: &mut Vec<EntityTypeName>) {
    match expr {
        TypeExpr::Wildcard => {}
        TypeExpr::Type(iri) => out.extend(iri.as_str().parse::<EntityTypeName>().ok()),
        TypeExpr::And(children) | TypeExpr::Or(children) => {
            for child in children {
                collect_selection(child, out);
            }
        }
    }
}

/// Every type guard a `q` filter's linked-entity traversals carry.
fn collect_guards(expr: &Expr, out: &mut Vec<EntityTypeName>) {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => {
            for term in terms {
                collect_guards(term, out);
            }
        }
        Expr::Pred(pred) => collect_path_guards(pred.path(), out),
    }
}

/// The guard types along one attribute axis.
fn collect_path_guards(path: &QueryPath, out: &mut Vec<EntityTypeName>) {
    match path {
        QueryPath::Local(_) => {}
        QueryPath::Linked { guard_types, inner, .. } => {
            out.extend(guard_types.iter().filter_map(|guard| guard.as_str().parse::<EntityTypeName>().ok()));
            collect_path_guards(inner, out);
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::projected_type_names::projected_type_names;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::{
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser,
        type_selection::grammar::TypeExpr,
    };
    use iri_rs::IriBuf;

    fn selection_with(type_expr: Option<TypeExpr>, q: Option<&str>) -> EntitySelection {
        let mut selection = EntitySelection::default();
        selection.selectors.push(EntitySelector {
            type_expr,
            ..EntitySelector::default()
        });
        selection.q_expr = q.map(|q| parser::parse(q).expect("parse q"));
        selection
    }

    fn texts(names: &[EntityTypeName]) -> Vec<String> {
        let mut texts: Vec<String> = names.iter().map(EntityTypeName::as_str).map(str::to_string).collect();
        texts.sort();
        texts
    }

    #[test]
    fn every_type_of_a_boolean_type_selection_is_collected() {
        let expr = TypeExpr::Or(vec![
            TypeExpr::And(vec![
                TypeExpr::Type(IriBuf::new("https://example.com/Home".to_string()).expect("iri")),
                TypeExpr::Type(IriBuf::new("https://example.com/Vehicle".to_string()).expect("iri")),
            ]),
            TypeExpr::Wildcard,
        ]);
        pretty_assertions::assert_eq!(
            texts(&projected_type_names(&selection_with(Some(expr), None))),
            vec!["https://example.com/Home".to_string(), "https://example.com/Vehicle".to_string()]
        );
    }

    #[test]
    fn the_type_guards_of_a_linked_traversal_are_collected() {
        let selection = selection_with(
            None,
            Some("\"https://example.com/outer\"{\"https://example.com/A\",\"https://example.com/B\":\"https://example.com/zone\"}==\"north\""),
        );
        pretty_assertions::assert_eq!(
            texts(&projected_type_names(&selection)),
            vec!["https://example.com/A".to_string(), "https://example.com/B".to_string()]
        );
    }

    #[test]
    fn a_selection_without_types_or_guards_resolves_nothing() {
        assert_eq!(
            projected_type_names(&selection_with(None, Some("\"https://example.com/speed\">1"))),
            [] as [EntityTypeName; 0]
        );
    }
}
