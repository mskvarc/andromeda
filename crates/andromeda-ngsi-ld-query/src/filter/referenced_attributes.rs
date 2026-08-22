//! The attributes a `q` filter names (clause 4.9), read off its predicates.
//!
//! Clause 5.10.2.4 matches the attributes a discovery query's `q` mentions
//! against registrations exactly as it matches an explicit `attrs` list, so a
//! consumer needs the filter's attribute footprint without evaluating it.

use crate::filter::{
    expression::Expr,
    path::{AttrPath, QueryPath},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The top-level attribute every predicate of `expr` addresses, in first-seen
/// order and without repetition.
///
/// A wholly local path names its own attribute. A linked-entity hop names the
/// Relationship it descends through: that is the member of the entity under
/// evaluation the predicate reads, and the attribute a context source would
/// have to hold for the hop to begin.
pub fn referenced_attributes(expr: &Expr) -> Vec<&AttributeName> {
    let mut names = Vec::new();
    collect(expr, &mut names);
    names
}

/// Walk `expr`, pushing each predicate's top-level attribute the first time it
/// is seen.
fn collect<'expr>(expr: &'expr Expr, names: &mut Vec<&'expr AttributeName>) {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter().for_each(|term| collect(term, names)),
        Expr::Pred(pred) => {
            let name = top_level_attribute(pred.path());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
}

/// The attribute of the entity under evaluation that an axis starts from.
const fn top_level_attribute(path: &QueryPath) -> &AttributeName {
    match path {
        QueryPath::Local(AttrPath { attr, .. }) => attr,
        QueryPath::Linked { relationship, .. } => relationship,
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{parser, referenced_attributes::referenced_attributes};

    fn names(q: &str) -> Vec<String> {
        let expr = parser::parse(q).expect("valid q");
        referenced_attributes(&expr).into_iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_single_predicate_names_its_attribute() {
        pretty_assertions::assert_eq!(names("speed>10"), vec!["speed"]);
    }

    #[test]
    fn a_sub_attribute_or_compound_member_path_names_only_the_top_level_attribute() {
        pretty_assertions::assert_eq!(names("speed.accuracy>10"), vec!["speed"]);
        pretty_assertions::assert_eq!(names("address[city]==\"Berlin\""), vec!["address"]);
    }

    #[test]
    fn conjunctions_and_disjunctions_collect_every_attribute_once_in_order() {
        pretty_assertions::assert_eq!(names("speed>10;(color==\"red\"|speed<50);brand"), vec!["speed", "color", "brand"]);
    }

    /// Clause 4.9: a hop descends through a Relationship of the entity under
    /// evaluation, so that Relationship is the attribute the filter reads.
    #[test]
    fn a_linked_entity_hop_names_the_relationship_it_descends_through() {
        pretty_assertions::assert_eq!(names("isParkedIn{name==\"Lot\"}"), vec!["isParkedIn"]);
    }
}
