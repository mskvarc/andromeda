//! The projection list in force for one entity of a joined payload.
//!
//! The roots are narrowed by the request's list: `pick`, `omit`, both, or `attrs` as
//! the `pick` synonym that keeps `id` and `type` (Tables 6.4.3.2-1, 6.5.3.1-1
//! and 5.2.14.1-1). How far a list reaches depends on whether it holds a Linked
//! Entity Term:
//!
//! - A list without one "applies to all Entities in the payload in the case of
//!   Linked Entity retrieval" (clauses 5.7.1.5, 5.7.2.5 and 5.8.6), so every
//!   Linked Entity is narrowed by it.
//! - A list with one addresses the entity it was stated for: a Linked Entity
//!   Term `R{N}` names "an Attribute name within the Linked Entity" `R`
//!   reaches (clause 4.21), so that entity is narrowed by `N`, and `N`
//!   addresses it alone in turn. A Linked Entity no term addresses is not
//!   narrowed.
//!
//! Either way a target is reached only through an attribute returned under the
//! list: a bare `omit=R` removes `R`, so nothing is reached through it, while
//! `omit=R{N}` keeps `R` and gives the target `omit N`.

use crate::{keyword::Keyword, narrowing::list_terms::ListTerms};
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection};
use iri_rs::IriBuf;

/// The clause 4.21 list one entity of a joined payload is narrowed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListInForce<'p> {
    /// Nothing is narrowed.
    Unrestricted,
    /// A list holding no Linked Entity Term: it narrows every Entity in the
    /// payload (clauses 5.7.1.5, 5.7.2.5 and 5.8.6).
    Payload(ListTerms<'p>),
    /// A list holding a Linked Entity Term, or the group one names: it narrows
    /// the Entity it addresses and no other (clause 4.21).
    Addressed(ListTerms<'p>),
}

impl<'p> ListInForce<'p> {
    /// The list a request states for its roots: its clause 4.21 projection,
    /// or else its `attrs`, which the request validation keeps from being
    /// combined.
    #[must_use]
    pub fn of(projection: Option<&'p Projection>, attrs: &'p [IriBuf]) -> ListInForce<'p> {
        let terms = match projection {
            Some(Projection { pick, omit }) if !pick.is_empty() && !omit.is_empty() => ListTerms::PickAndOmit { pick, omit },
            Some(projection) if !projection.pick.is_empty() => ListTerms::Pick(&projection.pick),
            Some(projection) if !projection.omit.is_empty() => ListTerms::Omit(&projection.omit),
            Some(_) | None if !attrs.is_empty() => ListTerms::Attributes(attrs),
            Some(_) | None => return ListInForce::Unrestricted,
        };
        if terms.holds_a_group() {
            ListInForce::Addressed(terms)
        } else {
            ListInForce::Payload(terms)
        }
    }

    /// Whether the attribute keyed `key` is returned under this list.
    #[must_use]
    pub fn returns_attribute(&self, key: &str, index: &AttributeIriIndex) -> bool {
        match self {
            ListInForce::Unrestricted => true,
            ListInForce::Payload(terms) | ListInForce::Addressed(terms) => terms.returns_attribute(key, index),
        }
    }

    /// Whether the keyword member is kept under this list (clause 4.21).
    #[must_use]
    pub fn keeps_keyword(&self, keyword: Keyword) -> bool {
        match self {
            ListInForce::Unrestricted => true,
            ListInForce::Payload(terms) | ListInForce::Addressed(terms) => terms.keeps_keyword(keyword),
        }
    }

    /// The list in force for whatever the returned attribute keyed `key` links
    /// to.
    #[must_use]
    pub fn through(&self, key: &str, index: &AttributeIriIndex) -> ListInForce<'p> {
        match *self {
            ListInForce::Unrestricted | ListInForce::Payload(_) => *self,
            ListInForce::Addressed(terms) => terms.group(key, index).map_or(ListInForce::Unrestricted, ListInForce::Addressed),
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
    use crate::{
        keyword::Keyword,
        narrowing::{list::ListInForce, list_terms::ListTerms},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(key: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI")
    }

    fn index() -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in ["name", "email", "owner", "manager", "employer"] {
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
    fn a_request_list_without_a_linked_entity_term_reaches_the_payload() {
        let pick = Projection::builder().pick(vec![leaf("name"), leaf("owner")]).build();
        assert_eq!(ListInForce::of(Some(&pick), &[]), ListInForce::Payload(ListTerms::Pick(&pick.pick)));
        let attrs = [iri("name")];
        assert_eq!(ListInForce::of(None, &attrs), ListInForce::Payload(ListTerms::Attributes(&attrs)));
    }

    #[test]
    fn a_request_list_holding_a_linked_entity_term_addresses_its_roots() {
        let pick = Projection::builder()
            .pick(vec![ProjectionTerm::Id, nested("owner", vec![leaf("name")])])
            .build();
        assert_eq!(ListInForce::of(Some(&pick), &[]), ListInForce::Addressed(ListTerms::Pick(&pick.pick)));
        let omit = Projection::builder()
            .omit(vec![ProjectionTerm::Type, nested("owner", vec![leaf("name")])])
            .build();
        assert_eq!(ListInForce::of(Some(&omit), &[]), ListInForce::Addressed(ListTerms::Omit(&omit.omit)));
    }

    #[test]
    fn a_request_naming_pick_and_omit_narrows_by_both() {
        let projection = Projection::builder().pick(vec![leaf("name"), leaf("email")]).omit(vec![leaf("email")]).build();
        assert_eq!(
            ListInForce::of(Some(&projection), &[]),
            ListInForce::Payload(ListTerms::PickAndOmit {
                pick: &projection.pick,
                omit: &projection.omit
            })
        );
    }

    #[test]
    fn a_request_states_its_projection_before_its_attrs() {
        let projection = Projection::builder().omit(vec![leaf("email")]).build();
        let attrs = [iri("name")];
        assert_eq!(
            ListInForce::of(Some(&projection), &attrs),
            ListInForce::Payload(ListTerms::Omit(&projection.omit))
        );
        assert_eq!(
            ListInForce::of(Some(&Projection::default()), &attrs),
            ListInForce::Payload(ListTerms::Attributes(&attrs))
        );
    }

    /// Clause 5.7.1.5: a plain list "applies to all Entities in the payload".
    #[test]
    fn a_payload_list_is_in_force_through_every_relationship() {
        let pick = [leaf("name"), leaf("owner")];
        let list = ListInForce::Payload(ListTerms::Pick(&pick));
        assert_eq!(list.through("owner", &index()), list);
        let omit = [leaf("email")];
        let list = ListInForce::Payload(ListTerms::Omit(&omit));
        assert_eq!(list.through("owner", &index()), list);
    }

    /// Clause 4.21: `R{N}` addresses `N` to the entity `R` reaches.
    #[test]
    fn through_a_linked_entity_term_its_group_addresses_the_target() {
        let pick = [nested("owner", vec![leaf("name")])];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Pick(&pick)).through("owner", &index()),
            ListInForce::Addressed(ListTerms::Pick(&[leaf("name")]))
        );
        let omit = [nested("owner", vec![leaf("email")])];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Omit(&omit)).through("owner", &index()),
            ListInForce::Addressed(ListTerms::Omit(&[leaf("email")]))
        );
    }

    /// The plain terms of a list holding a group address its own entity only,
    /// so a target reached through a Relationship it names bare is whole.
    #[test]
    fn through_a_relationship_no_group_addresses_the_target_is_unrestricted() {
        let pick = [nested("owner", vec![leaf("name")]), leaf("manager")];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Pick(&pick)).through("manager", &index()),
            ListInForce::Unrestricted
        );
        let omit = [ProjectionTerm::Type, nested("owner", vec![leaf("name")])];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Omit(&omit)).through("manager", &index()),
            ListInForce::Unrestricted
        );
    }

    /// A group addresses the one entity its Relationship reaches: it is not
    /// carried on to the entities reached from there.
    #[test]
    fn a_group_without_its_own_group_leaves_the_next_level_unrestricted() {
        let group = [leaf("name"), leaf("employer")];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Pick(&group)).through("employer", &index()),
            ListInForce::Unrestricted
        );
        let omitted = [leaf("name")];
        assert_eq!(
            ListInForce::Addressed(ListTerms::Omit(&omitted)).through("employer", &index()),
            ListInForce::Unrestricted
        );
    }

    #[test]
    fn nested_groups_address_one_level_each() {
        let pick = [nested("owner", vec![leaf("name"), nested("employer", vec![leaf("email")])])];
        let owner = ListInForce::Addressed(ListTerms::Pick(&pick)).through("owner", &index());
        assert_eq!(owner.through("employer", &index()), ListInForce::Addressed(ListTerms::Pick(&[leaf("email")])));
    }

    #[test]
    fn the_keyword_rule_follows_the_group() {
        let without = [nested("owner", vec![leaf("name")])];
        let with = [nested("owner", vec![ProjectionTerm::Id, leaf("name")])];
        assert!(
            !ListInForce::Addressed(ListTerms::Pick(&without))
                .through("owner", &index())
                .keeps_keyword(Keyword::Id)
        );
        assert!(
            ListInForce::Addressed(ListTerms::Pick(&with))
                .through("owner", &index())
                .keeps_keyword(Keyword::Id)
        );
    }

    #[test]
    fn an_unrestricted_list_returns_everything() {
        let list = ListInForce::of(None, &[]);
        assert!(list.is_unrestricted());
        assert!(list.returns_attribute("anything", &AttributeIriIndex::default()));
        assert!(list.keeps_keyword(Keyword::Scope));
        assert_eq!(list.through("owner", &index()), ListInForce::Unrestricted);
    }
}
