//! Which Attributes of a matching Entity a response can hold, read off the
//! request's `attrs` or clause 4.21 projection.
//!
//! A store reads only these Attributes of the Entities it answers with. What
//! reaches the response is still decided by the narrowing the rendering
//! applies, so this is never narrower than it: an Attribute named here may
//! still be dropped there (a sub-projection inside a Linked Entity), never the
//! reverse.
//!
//! - `attrs` names the Attributes kept (clause 5.7.2), as a `pick` naming only
//!   Attributes would; the request validation keeps it from being combined
//!   with `pick` or `omit`.
//! - A `pick` keeps the Attributes its terms name. A Linked Entity Term
//!   `R{N}` keeps `R` itself, since the Linked Entity is reached through it.
//! - An `omit` takes away the Attributes its bare terms name. `omit=R{N}`
//!   removes `N` inside the Linked Entity and keeps `R`.
//! - Beside a `pick`, a bare `omit` term takes its Attribute out of what the
//!   `pick` keeps.

use crate::projection::{expanded_projection::ExpandedProjection, expanded_projection_term::ExpandedProjectionTerm};
use iri_rs::IriBuf;

/// The Attributes a response to a request can hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnedAttributes<'q> {
    /// Every Attribute.
    Every,
    /// The Attributes named, and no other; none when the list is empty.
    Named(Vec<&'q IriBuf>),
    /// Every Attribute but those named.
    AllBut(Vec<&'q IriBuf>),
}

impl<'q> ReturnedAttributes<'q> {
    /// The Attributes `projection`, or else `attrs`, lets a response hold.
    #[must_use]
    pub fn of(projection: &'q ExpandedProjection, attrs: &'q [IriBuf]) -> ReturnedAttributes<'q> {
        let bare_omitted: Vec<&IriBuf> = projection
            .omit
            .iter()
            .filter(|term| term.linked_terms().is_empty())
            .filter_map(ExpandedProjectionTerm::attribute_iri)
            .collect();
        if !projection.pick.is_empty() {
            let picked = projection
                .pick
                .iter()
                .filter_map(ExpandedProjectionTerm::attribute_iri)
                .filter(|iri| !bare_omitted.contains(iri))
                .collect();
            return ReturnedAttributes::Named(picked);
        }
        if !projection.omit.is_empty() {
            return ReturnedAttributes::AllBut(bare_omitted);
        }
        if !attrs.is_empty() {
            return ReturnedAttributes::Named(attrs.iter().collect());
        }
        ReturnedAttributes::Every
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::{
        expanded_projection::ExpandedProjection,
        expanded_projection_term::ExpandedProjectionTerm,
        returned_attributes::ReturnedAttributes,
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attribute(name: &str) -> ExpandedProjectionTerm {
        ExpandedProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    fn linked(name: &str, inner: &str) -> ExpandedProjectionTerm {
        ExpandedProjectionTerm::Attribute {
            iri: iri(name),
            linked: vec![attribute(inner)],
        }
    }

    #[test]
    fn a_request_naming_no_list_returns_every_attribute() {
        assert_eq!(ReturnedAttributes::of(&ExpandedProjection::default(), &[]), ReturnedAttributes::Every);
    }

    #[test]
    fn attrs_returns_the_attributes_it_names() {
        let attrs = [iri("a"), iri("b")];
        assert_eq!(
            ReturnedAttributes::of(&ExpandedProjection::default(), &attrs),
            ReturnedAttributes::Named(vec![&attrs[0], &attrs[1]])
        );
    }

    /// Clause 4.21: `R{N}` reaches the Linked Entity through `R`, so `R` is
    /// returned; the keywords name no Attribute.
    #[test]
    fn a_pick_returns_its_attributes_and_the_relationship_of_a_linked_entity_term() {
        let projection = ExpandedProjection::builder()
            .pick(vec![ExpandedProjectionTerm::Id, attribute("a"), linked("owner", "name")])
            .build();
        assert_eq!(
            ReturnedAttributes::of(&projection, &[]),
            ReturnedAttributes::Named(vec![&iri("a"), &iri("owner")])
        );
    }

    #[test]
    fn a_pick_of_keywords_alone_returns_no_attribute() {
        let projection = ExpandedProjection::builder()
            .pick(vec![ExpandedProjectionTerm::Id, ExpandedProjectionTerm::Type])
            .build();
        assert_eq!(ReturnedAttributes::of(&projection, &[]), ReturnedAttributes::Named(Vec::new()));
    }

    /// Clause 4.21: `omit=R{N}` removes `N` inside the Linked Entity, not `R`.
    #[test]
    fn an_omit_returns_every_attribute_but_its_bare_terms() {
        let projection = ExpandedProjection::builder()
            .omit(vec![ExpandedProjectionTerm::Type, attribute("a"), linked("owner", "name")])
            .build();
        assert_eq!(ReturnedAttributes::of(&projection, &[]), ReturnedAttributes::AllBut(vec![&iri("a")]));
    }

    #[test]
    fn beside_a_pick_a_bare_omit_takes_its_attribute_out() {
        let projection = ExpandedProjection::builder()
            .pick(vec![attribute("a"), attribute("b"), linked("owner", "name")])
            .omit(vec![attribute("b"), linked("owner", "name")])
            .build();
        assert_eq!(
            ReturnedAttributes::of(&projection, &[]),
            ReturnedAttributes::Named(vec![&iri("a"), &iri("owner")])
        );
    }
}
