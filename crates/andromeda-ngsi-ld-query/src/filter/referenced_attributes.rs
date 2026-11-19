//! The attributes a `q` filter names (clause 4.9), read off its query terms.
//!
//! Clause 5.10.2.4 matches the attributes a discovery query's `q` mentions
//! against registrations exactly as it matches an explicit `attrs` list, so a
//! consumer needs the filter's attribute footprint without evaluating it.

use crate::{
    filter::{Filter, attribute_path::AttributePath, value_path::ValuePath},
    name_stage::NameStage,
};

/// The top-level attribute every query term of `filter` addresses, in first-seen
/// order and without repetition.
///
/// A wholly local value path names its own attribute. A linked-entity hop names the
/// Relationship it descends through: that is the member of the entity under
/// evaluation the query term reads, and the attribute a context source would
/// have to hold for the hop to begin.
#[must_use]
pub fn referenced_attributes<S: NameStage>(filter: &Filter<S>) -> Vec<&S::Attribute> {
    let mut names = Vec::new();
    collect(filter, &mut names);
    names
}

/// Walk `filter`, pushing each query term's top-level attribute the first time it
/// is seen.
fn collect<'filter, S: NameStage>(filter: &'filter Filter<S>, names: &mut Vec<&'filter S::Attribute>) {
    match filter {
        Filter::And(filters) | Filter::Or(filters) => filters.iter().for_each(|filter| collect(filter, names)),
        Filter::Term(term) => {
            let name = top_level_attribute(term.path());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
}

/// The attribute of the entity under evaluation that an attribute path starts from.
const fn top_level_attribute<S: NameStage>(path: &AttributePath<S>) -> &S::Attribute {
    match path {
        AttributePath::Local(ValuePath { attribute, .. }) => attribute,
        AttributePath::Linked { relationship, .. } => relationship,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{Filter, referenced_attributes::referenced_attributes},
        name_stage::Written,
    };

    fn names(q: &str) -> Vec<String> {
        let filter = q.parse::<Filter<Written>>().expect("valid q");
        referenced_attributes(&filter).into_iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_single_term_names_its_attribute() {
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
        pretty_assertions::assert_eq!(names("isParkedIn{name}==\"Lot\""), vec!["isParkedIn"]);
    }
}
