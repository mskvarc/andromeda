//! The attributes every match of a filter must hold.

use crate::queried_entity::attribute_reading::QueriedEntity;
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::filter::reading::{filter_reading::FilterReading, term_reading::TermReading};

/// The attributes an entity must hold for a filter to hold over it (clause
/// 4.9).
///
/// Every Query Term is existential over the targets its path addresses, and a
/// target is a value of an instance of the term's attribute, so an entity
/// without that attribute satisfies no term over it, `!=` and `!~=` included.
/// A term behind a hop reads the Relationship of the entity itself first, so
/// that Relationship is what it requires. A conjunction requires what each
/// conjunct requires; a disjunction requires none of its names, for any one
/// branch may hold alone.
///
/// The set is read off the entity as it stands and never off a change, so it
/// belongs to the evaluation of one state and to no index of what changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequiredAttributes {
    /// The expanded names, each once.
    names: Vec<AttributeIri>,
}

impl RequiredAttributes {
    /// The attributes every entity `filter` holds for must hold.
    #[must_use]
    pub fn of(filter: &FilterReading) -> RequiredAttributes {
        let mut names = Vec::new();
        collect(filter, &mut names);
        names.sort();
        names.dedup();
        RequiredAttributes { names }
    }

    /// The expanded names, in order.
    #[must_use]
    pub const fn names(&self) -> &[AttributeIri] {
        self.names.as_slice()
    }

    /// Whether checking the attributes before the terms saves work: with one
    /// attribute the first term reads it anyway, so a check would only read
    /// it twice.
    #[must_use]
    pub const fn saves_work(&self) -> bool {
        self.names.len() > 1
    }

    /// Whether `entity` holds every attribute: a state failing this satisfies
    /// no filter these attributes were read from.
    pub fn held_by<'a, E: QueriedEntity<'a>>(&self, entity: E) -> bool {
        self.names.iter().all(|name| entity.attribute_instances(name).next().is_some())
    }
}

/// Adds the attributes the conjuncts of `filter` require to `names`.
fn collect(filter: &FilterReading, names: &mut Vec<AttributeIri>) {
    match filter {
        FilterReading::All(children) => children.iter().for_each(|child| collect(child, names)),
        FilterReading::Any(_) => {}
        FilterReading::Term(term) => names.push(required_by(term)),
    }
}

/// The attribute of the entity under evaluation `term` reads first.
fn required_by(term: &TermReading) -> AttributeIri {
    term.hops().first().map_or(term.attribute(), |hop| hop.relationship()).clone()
}

#[cfg(test)]
mod tests {
    use crate::q::required_attributes::RequiredAttributes;
    use andromeda_ngsi_ld_query::{
        filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading},
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
    };

    const EXAMPLE_A: &str = "https://example.com/a";
    const EXAMPLE_B: &str = "https://example.com/b";
    const EXAMPLE_C: &str = "https://example.com/c";

    /// The names `q` requires.
    fn required(q: &str) -> Vec<String> {
        let reading = FilterReading::of(
            &EXAMPLE_NAMESPACE
                .expanded(q.parse::<Filter<Written>>().expect("a clause 4.9 filter"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        );
        RequiredAttributes::of(&reading).names().iter().map(|name| name.as_str().to_owned()).collect()
    }

    #[test]
    fn a_conjunction_requires_the_attribute_of_each_conjunct() {
        assert_eq!(required("a==1;b!=2;c"), [EXAMPLE_A, EXAMPLE_B, EXAMPLE_C]);
        assert_eq!(required("a==1;a>0;(b;c.s)"), [EXAMPLE_A, EXAMPLE_B, EXAMPLE_C]);
    }

    #[test]
    fn a_disjunction_requires_none_of_its_names() {
        assert_eq!(required("a==1|b"), Vec::<String>::new());
        assert_eq!(required("d;(a==1|b)"), ["https://example.com/d"]);
    }

    #[test]
    fn a_hop_requires_the_relationship_of_the_entity_itself() {
        assert_eq!(required("r{a}==1;b"), [EXAMPLE_B, "https://example.com/r"]);
    }
}
