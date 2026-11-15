//! Which entity type names a compiled query has to resolve against the
//! `entity_type_names` dictionary before its SQL can be rendered.
//!
//! Two parts of a query test entity types by dictionary key: the type
//! selection of every entity selector (clause 4.17) and the type guard of a
//! linked-entity traversal inside a `q` filter (clause 4.9). Both are
//! collected here so the read path resolves them in one lookup.

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::reading::filter_reading::FilterReading, type_selection::grammar::TypeExpr};

/// Collect the type names a query tests by dictionary key.
///
/// The type selection carries expanded IRIs, the text the dictionary stores,
/// which satisfy the clause 4.6.2 grammar an [`EntityTypeName`] validates, so
/// one that nevertheless fails to parse is left out and its predicate later
/// compiles as an unresolved, and therefore unsatisfiable, type. A traversal
/// guard is read from the `q` reading, which holds its types as type names.
#[must_use]
pub fn projected_type_names(selection: &EntitySelection) -> Vec<EntityTypeName> {
    let mut names = Vec::new();
    for selector in &selection.selectors {
        if let Some(type_expr) = &selector.type_expr {
            collect_selection(type_expr, &mut names);
        }
    }
    if let Some(q) = &selection.q {
        collect_guards(q.reading(), &mut names);
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

/// Every type guard the linked-entity traversals of a `q` filter's reading
/// carry.
fn collect_guards(reading: &FilterReading, out: &mut Vec<EntityTypeName>) {
    for term in reading.terms() {
        // clone: the caller owns the resolved list, which outlives the reading
        out.extend(term.hops().iter().flat_map(|hop| hop.guard_types().iter().cloned()));
    }
}

#[cfg(test)]
mod tests {
    use crate::projected_type_names::projected_type_names;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::{
        entity_selection::{EntitySelection, EntitySelector},
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser},
        type_selection::grammar::TypeExpr,
    };
    use iri_rs::IriBuf;

    fn selection_with(type_expr: Option<TypeExpr>, q: Option<&str>) -> EntitySelection {
        let mut selection = EntitySelection::default();
        selection.selectors.push(EntitySelector {
            type_expr,
            ..EntitySelector::default()
        });
        selection.q = q.map(|q| CompiledFilter::new(parser::parse(q).expect("parse q"), LanguageMatching::TagEquality));
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
