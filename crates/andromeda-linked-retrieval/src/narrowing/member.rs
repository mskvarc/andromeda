//! What a writer asks for every member of a Linked Entity it writes.

use crate::{keyword::Keyword, narrowing::list::ListInForce, representation::LinkedRepresentation};
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use derive_more::IsVariant;

/// Whether a writer writes one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum MemberDecision {
    /// The member is written.
    Write,
    /// The member is left out.
    Skip,
}

/// The decision over every member of one Linked Entity: the list in force for
/// it, and the index its attribute keys are matched through.
#[derive(Debug, Clone, Copy)]
pub struct MemberRule<'p> {
    /// The list in force for the entity.
    list: ListInForce<'p>,
    /// The expansion of every attribute key the payload carries.
    index: &'p AttributeIriIndex,
}

impl<'p> MemberRule<'p> {
    /// The rule `list` states, matched through `index`.
    #[must_use]
    pub const fn new(list: ListInForce<'p>, index: &'p AttributeIriIndex) -> MemberRule<'p> {
        MemberRule { list, index }
    }

    /// Whether the attribute keyed `key` is written.
    #[must_use]
    pub fn member(&self, key: &str) -> MemberDecision {
        if self.list.returns_attribute(key, self.index) {
            MemberDecision::Write
        } else {
            MemberDecision::Skip
        }
    }

    /// Whether the keyword member is written.
    #[must_use]
    pub fn keyword(&self, keyword: Keyword) -> MemberDecision {
        if self.list.keeps_keyword(keyword) {
            MemberDecision::Write
        } else {
            MemberDecision::Skip
        }
    }

    /// The rule, when it narrows anything at all.
    #[must_use]
    pub const fn restricting(self) -> Option<MemberRule<'p>> {
        match self.list {
            ListInForce::Unrestricted => None,
            ListInForce::Payload(_) | ListInForce::Addressed(_) => Some(self),
        }
    }

    /// Whether the rule leaves `node` no member at all (no Attribute and none
    /// of `id`, `type` and `scope`), which keeps it out of the payload.
    #[must_use]
    pub fn empties<R: LinkedRepresentation>(&self, node: &R::Node) -> bool {
        !R::keyword_members(node).iter().any(|keyword| self.list.keeps_keyword(keyword))
            && !R::attribute_keys(node).any(|key| self.list.returns_attribute(key, self.index))
    }

    /// The list in force.
    #[must_use]
    pub const fn list(&self) -> ListInForce<'p> {
        self.list
    }

    /// Where the attribute keyed `key` stands in the list naming what is kept,
    /// if the rule's list names one.
    #[must_use]
    pub fn pick_position(&self, key: &str) -> Option<usize> {
        self.list.pick_position(key, self.index)
    }

    /// The rule for whatever the returned attribute keyed `key` links to.
    #[must_use]
    pub fn through(&self, key: &str) -> MemberRule<'p> {
        MemberRule {
            list: self.list.through(key, self.index),
            index: self.index,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        keyword::Keyword,
        narrowing::{
            list::ListInForce,
            list_terms::ListTerms,
            member::{MemberDecision, MemberRule},
        },
        test_graph::{Nodes, node},
    };
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn size() -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new("https://example.com/size".to_owned()).expect("valid IRI"),
            linked: Vec::new(),
        }
    }

    #[test]
    fn an_unrestricted_rule_restricts_nothing() {
        let index = AttributeIriIndex::default();
        assert!(MemberRule::new(ListInForce::Unrestricted, &index).restricting().is_none());
        let pick = [ProjectionTerm::Id];
        assert!(MemberRule::new(ListInForce::Payload(ListTerms::Pick(&pick)), &index).restricting().is_some());
    }

    #[test]
    fn a_pick_writes_only_what_it_names() {
        let index = AttributeIriIndex::default();
        let pick = [ProjectionTerm::Id];
        let rule = MemberRule::new(ListInForce::Payload(ListTerms::Pick(&pick)), &index);
        assert_eq!(rule.keyword(Keyword::Id), MemberDecision::Write);
        assert_eq!(rule.keyword(Keyword::Type), MemberDecision::Skip);
        assert_eq!(rule.member("name"), MemberDecision::Skip);
    }

    /// A list naming none of the node's members empties it.
    #[test]
    fn a_list_naming_none_of_the_members_empties_the_node() {
        let index = AttributeIriIndex::default();
        let pick = [size()];
        let city = node("C", &[]).with_properties(&["name"]);
        assert!(MemberRule::new(ListInForce::Payload(ListTerms::Pick(&pick)), &index).empties::<Nodes>(&city));
        let keeping_id = [ProjectionTerm::Id, size()];
        assert!(!MemberRule::new(ListInForce::Payload(ListTerms::Pick(&keeping_id)), &index).empties::<Nodes>(&city));
    }
}
