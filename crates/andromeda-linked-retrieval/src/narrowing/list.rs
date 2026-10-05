//! The projection list in force for one entity of a joined payload.
//!
//! The roots are narrowed by the request's list: `pick`, `omit`, or `attrs` as
//! the `pick` synonym that keeps `id` and `type` (Tables 6.4.3.2-1, 6.5.3.1-1
//! and 5.2.14.1-1). A target reached from an entity through an attribute `R`
//! returned under that entity's list `L` is narrowed by the group `L` gives
//! `R` when it names it as `R{N}` (clause 4.21), and by `L` itself otherwise,
//! because the list "applies to all Entities in the payload in the case of
//! Linked Entity retrieval" (clauses 5.7.1.5, 5.7.2.5 and 5.8.6). `omit`
//! follows the same shape: a bare `omit=R` removes `R`, so nothing is reached
//! through it, and `omit=R{N}` keeps `R` and gives the target `omit N`.

use crate::keyword::Keyword;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, matching::matching_term, term::ProjectionTerm};
use iri_rs::IriBuf;

/// The clause 4.21 list one entity of a joined payload is narrowed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListInForce<'p> {
    /// Nothing is narrowed.
    Unrestricted,
    /// Only the named members are kept.
    Pick(&'p [ProjectionTerm]),
    /// The named members are removed.
    Omit(&'p [ProjectionTerm]),
    /// `attrs`: only the named Attributes are kept, and every keyword member.
    Attributes(&'p [IriBuf]),
}

impl<'p> ListInForce<'p> {
    /// The list a request states for its roots: its clause 4.21 projection,
    /// or else its `attrs`, which the request validation keeps from being
    /// combined.
    #[must_use]
    pub fn of(projection: Option<&'p Projection>, attrs: &'p [IriBuf]) -> ListInForce<'p> {
        match projection {
            Some(projection) if !projection.pick.is_empty() => ListInForce::Pick(&projection.pick),
            Some(projection) if !projection.omit.is_empty() => ListInForce::Omit(&projection.omit),
            Some(_) | None if !attrs.is_empty() => ListInForce::Attributes(attrs),
            Some(_) | None => ListInForce::Unrestricted,
        }
    }

    /// Whether the attribute keyed `key` is returned under this list.
    #[must_use]
    pub fn returns_attribute(&self, key: &str, index: &AttributeIriIndex) -> bool {
        match self {
            ListInForce::Unrestricted => true,
            ListInForce::Pick(terms) => matching_term(key, terms, index).is_some(),
            // Clause 4.21: `omit=R{N}` omits `N` inside what `R` links to, so
            // only a bare term removes `R` itself.
            ListInForce::Omit(terms) => matching_term(key, terms, index).is_none_or(|term| !term.linked_terms().is_empty()),
            ListInForce::Attributes(iris) => match index.expanded(key) {
                Some(expanded) => iris.contains(expanded),
                None => iris.iter().any(|iri| iri.as_str() == key),
            },
        }
    }

    /// Whether the keyword member is kept under this list (clause 4.21).
    #[must_use]
    pub fn keeps_keyword(&self, keyword: Keyword) -> bool {
        match self {
            ListInForce::Unrestricted | ListInForce::Attributes(_) => true,
            ListInForce::Pick(terms) => terms.contains(&keyword.term()),
            ListInForce::Omit(terms) => !terms.contains(&keyword.term()),
        }
    }

    /// The list in force for whatever the returned attribute keyed `key` links
    /// to.
    #[must_use]
    pub fn through(&self, key: &str, index: &AttributeIriIndex) -> ListInForce<'p> {
        let group = |terms: &'p [ProjectionTerm]| {
            matching_term(key, terms, index)
                .map(ProjectionTerm::linked_terms)
                .filter(|group| !group.is_empty())
        };
        match *self {
            ListInForce::Pick(terms) => group(terms).map_or(*self, ListInForce::Pick),
            ListInForce::Omit(terms) => group(terms).map_or(*self, ListInForce::Omit),
            ListInForce::Unrestricted | ListInForce::Attributes(_) => *self,
        }
    }

    /// Whether this list narrows nothing.
    #[must_use]
    pub const fn is_unrestricted(&self) -> bool {
        matches!(self, ListInForce::Unrestricted)
    }
}

#[cfg(test)]
mod tests {
    use crate::{keyword::Keyword, narrowing::list::ListInForce};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, term::ProjectionTerm};
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
        let list = ListInForce::Pick(&pick);
        assert!(list.returns_attribute("name", &index()));
        assert!(!list.returns_attribute("email", &index()));
    }

    #[test]
    fn an_omit_removes_a_bare_name_and_keeps_a_nested_one() {
        let omit = [leaf("email"), nested("owner", vec![leaf("name")])];
        let list = ListInForce::Omit(&omit);
        assert!(!list.returns_attribute("email", &index()));
        assert!(list.returns_attribute("owner", &index()));
    }

    #[test]
    fn through_a_nested_term_gives_its_group() {
        let pick = [nested("owner", vec![leaf("name")])];
        assert_eq!(ListInForce::Pick(&pick).through("owner", &index()), ListInForce::Pick(&[leaf("name")]));
    }

    /// Clause 5.7.1.5: the list "applies to all Entities in the payload".
    #[test]
    fn through_a_bare_term_inherits_the_list() {
        let pick = [leaf("name"), leaf("owner")];
        assert_eq!(ListInForce::Pick(&pick).through("owner", &index()), ListInForce::Pick(&pick));
    }

    #[test]
    fn through_an_omit_not_naming_the_relationship_inherits_the_list() {
        let omit = [leaf("email")];
        assert_eq!(ListInForce::Omit(&omit).through("owner", &index()), ListInForce::Omit(&omit));
    }

    #[test]
    fn through_a_nested_omit_gives_its_group() {
        let omit = [nested("owner", vec![leaf("email")])];
        assert_eq!(ListInForce::Omit(&omit).through("owner", &index()), ListInForce::Omit(&[leaf("email")]));
    }

    /// Clause 4.21: a group keeps `id` only when it names it.
    #[test]
    fn the_keyword_rule_follows_the_list() {
        let without = [nested("owner", vec![leaf("name")])];
        let with = [nested("owner", vec![ProjectionTerm::Id, leaf("name")])];
        assert!(!ListInForce::Pick(&without).through("owner", &index()).keeps_keyword(Keyword::Id));
        assert!(ListInForce::Pick(&with).through("owner", &index()).keeps_keyword(Keyword::Id));
    }

    /// Table 6.5.3.1-1: `attrs` is the `pick` synonym that keeps the identity.
    #[test]
    fn attrs_keeps_identity() {
        let attrs = [iri("name")];
        let list = ListInForce::Attributes(&attrs);
        assert!(list.keeps_keyword(Keyword::Id));
        assert!(list.keeps_keyword(Keyword::Type));
        assert!(list.returns_attribute("name", &index()));
        assert!(!list.returns_attribute("email", &index()));
    }

    #[test]
    fn an_unrestricted_list_returns_everything() {
        let list = ListInForce::of(None, &[]);
        assert!(list.is_unrestricted());
        assert!(list.returns_attribute("anything", &AttributeIriIndex::default()));
        assert!(list.keeps_keyword(Keyword::Scope));
    }

    #[test]
    fn a_request_states_its_projection_before_its_attrs() {
        let projection = Projection::builder().omit(vec![leaf("email")]).build();
        let attrs = [iri("name")];
        assert_eq!(ListInForce::of(Some(&projection), &attrs), ListInForce::Omit(&projection.omit));
        assert_eq!(ListInForce::of(Some(&Projection::default()), &attrs), ListInForce::Attributes(&attrs));
    }
}
