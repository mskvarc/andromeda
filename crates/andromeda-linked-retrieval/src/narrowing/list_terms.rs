//! The terms of one clause 4.21 list, and what they keep of the entity they
//! narrow.

use crate::keyword::Keyword;
use andromeda_ngsi_ld_query::projection::{
    attribute_iri_index::AttributeIriIndex,
    matching::{matching_position, matching_term},
    term::ProjectionTerm,
};
use iri_rs::IriBuf;

/// The terms of a `pick`, `omit` or `attrs` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListTerms<'p> {
    /// Only the named members are kept.
    Pick(&'p [ProjectionTerm]),
    /// The named members are removed.
    Omit(&'p [ProjectionTerm]),
    /// `pick` and `omit` together: the picked members minus the omitted ones
    /// (clause 4.21 forbids neither beside the other).
    PickAndOmit {
        /// The members kept.
        pick: &'p [ProjectionTerm],
        /// The members then removed.
        omit: &'p [ProjectionTerm],
    },
    /// `attrs` or `notification.attributes`: only the named Attributes are
    /// kept, with `id` and `type`.
    Attributes(&'p [IriBuf]),
}

impl<'p> ListTerms<'p> {
    /// Whether the attribute keyed `key` is returned under these terms.
    #[must_use]
    pub fn returns_attribute(&self, key: &str, index: &AttributeIriIndex) -> bool {
        match self {
            ListTerms::Pick(terms) => matching_term(key, terms, index).is_some(),
            // Clause 4.21: `omit=R{N}` omits `N` inside what `R` links to, so
            // only a bare term removes `R` itself.
            ListTerms::Omit(terms) => matching_term(key, terms, index).is_none_or(|term| !term.linked_terms().is_empty()),
            ListTerms::PickAndOmit { pick, omit } => ListTerms::Pick(pick).returns_attribute(key, index) && ListTerms::Omit(omit).returns_attribute(key, index),
            ListTerms::Attributes(iris) => match index.expanded(key) {
                Some(expanded) => iris.contains(expanded),
                None => iris.iter().any(|iri| iri.as_str() == key),
            },
        }
    }

    /// Whether the keyword member is kept under these terms (clause 4.21).
    #[must_use]
    pub fn keeps_keyword(&self, keyword: Keyword) -> bool {
        match self {
            // Tables 6.5.3.1-1 and 5.2.14.1-1 make `attrs` and
            // `notification.attributes` "a synonym for pick, except that id,
            // type, scope are not allowed": `scope` goes, as under a `pick`
            // not naming it, while `id` and `type` stay to identify the
            // entity whatever Attributes it is narrowed to.
            ListTerms::Attributes(_) => keyword != Keyword::Scope,
            ListTerms::Pick(terms) => terms.contains(&keyword.term()),
            ListTerms::Omit(terms) => !terms.contains(&keyword.term()),
            ListTerms::PickAndOmit { pick, omit } => ListTerms::Pick(pick).keeps_keyword(keyword) && ListTerms::Omit(omit).keeps_keyword(keyword),
        }
    }

    /// The group these terms name for the attribute keyed `key` as its Linked
    /// Entity Term `key{N}`, as terms of the same kind.
    #[must_use]
    pub fn group(&self, key: &str, index: &AttributeIriIndex) -> Option<ListTerms<'p>> {
        let group = |terms: &'p [ProjectionTerm]| {
            matching_term(key, terms, index)
                .map(ProjectionTerm::linked_terms)
                .filter(|group| !group.is_empty())
        };
        match *self {
            ListTerms::Pick(terms) => group(terms).map(ListTerms::Pick),
            ListTerms::Omit(terms) => group(terms).map(ListTerms::Omit),
            ListTerms::PickAndOmit { pick, omit } => match (group(pick), group(omit)) {
                (Some(pick), Some(omit)) => Some(ListTerms::PickAndOmit { pick, omit }),
                (Some(pick), None) => Some(ListTerms::Pick(pick)),
                (None, Some(omit)) => Some(ListTerms::Omit(omit)),
                (None, None) => None,
            },
            ListTerms::Attributes(_) => None,
        }
    }

    /// Where the attribute keyed `key` stands in the list naming what is kept,
    /// or `None` when the terms name no such order (an `omit` alone, or a key
    /// the list does not name).
    #[must_use]
    pub fn pick_position(&self, key: &str, index: &AttributeIriIndex) -> Option<usize> {
        match self {
            ListTerms::Pick(pick) | ListTerms::PickAndOmit { pick, .. } => matching_position(key, pick, index),
            ListTerms::Omit(_) => None,
            ListTerms::Attributes(iris) => {
                let expanded = index.expanded(key);
                iris.iter().position(|iri| expanded.map_or(iri.as_str() == key, |expanded| iri == expanded))
            }
        }
    }

    /// Whether any term is a Linked Entity Term (clause 4.21).
    #[must_use]
    pub fn holds_a_group(&self) -> bool {
        match self {
            ListTerms::Pick(terms) | ListTerms::Omit(terms) => terms.iter().any(|term| !term.linked_terms().is_empty()),
            ListTerms::PickAndOmit { pick, omit } => ListTerms::Pick(pick).holds_a_group() || ListTerms::Omit(omit).holds_a_group(),
            ListTerms::Attributes(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{keyword::Keyword, narrowing::list_terms::ListTerms};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(key: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI")
    }

    fn index() -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in ["name", "email", "owner", "employer"] {
            index.insert(key.parse::<AttributeName>().expect("valid attribute name"), iri(key));
        }
        index
    }

    fn leaf(key: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(key),
            linked: Vec::new(),
        }
    }

    fn nested(key: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(key), linked }
    }

    #[test]
    fn a_pick_returns_only_named_attributes() {
        let pick = [leaf("name")];
        let terms = ListTerms::Pick(&pick);
        assert!(terms.returns_attribute("name", &index()));
        assert!(!terms.returns_attribute("email", &index()));
    }

    #[test]
    fn an_omit_removes_a_bare_name_and_keeps_a_nested_one() {
        let omit = [leaf("email"), nested("owner", vec![leaf("name")])];
        let terms = ListTerms::Omit(&omit);
        assert!(!terms.returns_attribute("email", &index()));
        assert!(terms.returns_attribute("owner", &index()));
    }

    #[test]
    fn a_nested_term_names_its_group_in_the_same_kind() {
        let pick = [nested("owner", vec![leaf("name")])];
        assert_eq!(ListTerms::Pick(&pick).group("owner", &index()), Some(ListTerms::Pick(&[leaf("name")])));
        let omit = [nested("owner", vec![leaf("email")])];
        assert_eq!(ListTerms::Omit(&omit).group("owner", &index()), Some(ListTerms::Omit(&[leaf("email")])));
    }

    #[test]
    fn a_bare_term_or_an_unnamed_attribute_names_no_group() {
        let pick = [leaf("owner"), nested("employer", vec![leaf("name")])];
        assert_eq!(ListTerms::Pick(&pick).group("owner", &index()), None);
        assert_eq!(ListTerms::Pick(&pick).group("name", &index()), None);
        assert_eq!(ListTerms::Attributes(&[iri("owner")]).group("owner", &index()), None);
    }

    #[test]
    fn only_a_linked_entity_term_makes_a_list_hold_a_group() {
        assert!(!ListTerms::Pick(&[leaf("name"), leaf("owner")]).holds_a_group());
        assert!(ListTerms::Omit(&[ProjectionTerm::Type, nested("owner", vec![leaf("name")])]).holds_a_group());
        assert!(!ListTerms::Attributes(&[iri("owner")]).holds_a_group());
    }

    /// Clause 4.21: a list keeps `id` only when it names it.
    #[test]
    fn the_keyword_rule_follows_the_terms() {
        assert!(!ListTerms::Pick(&[leaf("name")]).keeps_keyword(Keyword::Id));
        assert!(ListTerms::Pick(&[ProjectionTerm::Id, leaf("name")]).keeps_keyword(Keyword::Id));
        assert!(!ListTerms::Omit(&[ProjectionTerm::Id]).keeps_keyword(Keyword::Id));
    }

    /// Clause 4.21: `pick` with `omit` keeps the picked members minus the
    /// omitted ones, keywords included.
    #[test]
    fn pick_and_omit_together_keep_the_picked_members_minus_the_omitted_ones() {
        let pick = [ProjectionTerm::Id, ProjectionTerm::Type, leaf("name"), leaf("email")];
        let omit = [ProjectionTerm::Type, leaf("email")];
        let terms = ListTerms::PickAndOmit { pick: &pick, omit: &omit };
        assert!(terms.returns_attribute("name", &index()));
        assert!(!terms.returns_attribute("email", &index()));
        assert!(!terms.returns_attribute("owner", &index()));
        assert!(terms.keeps_keyword(Keyword::Id));
        assert!(!terms.keeps_keyword(Keyword::Type));
    }

    /// Each side of `pick` with `omit` hands its own group to the target.
    #[test]
    fn pick_and_omit_together_name_the_groups_of_both_sides() {
        let pick = [nested("owner", vec![leaf("name"), leaf("email")]), leaf("employer")];
        let omit = [nested("owner", vec![leaf("email")])];
        let terms = ListTerms::PickAndOmit { pick: &pick, omit: &omit };
        assert!(terms.holds_a_group());
        assert_eq!(
            terms.group("owner", &index()),
            Some(ListTerms::PickAndOmit {
                pick: &[leaf("name"), leaf("email")],
                omit: &[leaf("email")]
            })
        );
        assert_eq!(terms.group("employer", &index()), None);
    }

    /// Only a list naming what is kept orders its attributes.
    #[test]
    fn the_pick_position_follows_the_list_naming_what_is_kept() {
        let pick = [ProjectionTerm::Id, nested("owner", vec![leaf("name")]), leaf("employer")];
        let omit = [leaf("owner")];
        assert_eq!(ListTerms::Pick(&pick).pick_position("employer", &index()), Some(2));
        assert_eq!(ListTerms::PickAndOmit { pick: &pick, omit: &omit }.pick_position("owner", &index()), Some(1));
        assert_eq!(ListTerms::Pick(&pick).pick_position("email", &index()), None);
        assert_eq!(ListTerms::Omit(&omit).pick_position("owner", &index()), None);
        assert_eq!(ListTerms::Attributes(&[iri("email"), iri("owner")]).pick_position("owner", &index()), Some(1));
    }

    /// Table 6.5.3.1-1: `attrs` is the `pick` synonym that keeps the identity.
    #[test]
    fn attrs_keeps_identity_and_drops_scope() {
        let attrs = [iri("name")];
        let terms = ListTerms::Attributes(&attrs);
        assert!(terms.keeps_keyword(Keyword::Id));
        assert!(terms.keeps_keyword(Keyword::Type));
        assert!(!terms.keeps_keyword(Keyword::Scope));
        assert!(terms.returns_attribute("name", &index()));
        assert!(!terms.returns_attribute("email", &index()));
    }
}
