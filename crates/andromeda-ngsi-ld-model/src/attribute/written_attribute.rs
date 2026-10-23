//! The bare Attribute Fragment of a Partial Attribute Update or a Replace
//! Attribute (ETSI GS CIM 009 clauses 5.6.4 and 5.6.19) as a request wrote it.
//!
//! The fragment is the attribute itself, written without the name it stands
//! under: the request path carries that name, and the name decides part of
//! what the fragment is (clause 4.5.2.2 admits the seals only on the
//! `ngsildproof` Property). The body is therefore held as the JSON it was
//! written as, its top-level `@context` left out, until it is read under the
//! name.

use crate::{
    attribute::member_reading::MemberReading,
    node_refusal::{NodeReadError, RefusedNode},
    request_body::RequestBody,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::{
    compact_node::{aliased_node::AliasedNode, produced_node::ProducedNode},
    core_term_aliases::CoreTermAliases,
};
use serde::Deserialize;

/// A bare Attribute Fragment as a request wrote it, its `@context` left out.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct WrittenAttribute(JsonLiteral);

impl RequestBody for WrittenAttribute {}

impl WrittenAttribute {
    /// The member the fragment is, written for the attribute `attribute`, as a
    /// Context Producer wrote it under a context whose core term aliases are
    /// `aliases`: each member is read as the core term it stands for, and the
    /// system members of clause 5.2.2 it carries are not read.
    ///
    /// # Errors
    ///
    /// Returns the [`RefusedNode`] of a fragment no clause admits where an
    /// attribute stands, located from the fragment.
    pub fn read<M: MemberReading>(&self, attribute: &AttributeName, aliases: &CoreTermAliases) -> Result<M, RefusedNode> {
        match M::from_fragment_node(attribute.as_str(), ProducedNode::new(AliasedNode::new(self.0.piece(), aliases))) {
            Ok(member) => Ok(member),
            Err(NodeReadError::Refused(refused)) => Err(refused),
            Err(NodeReadError::Unreadable(never)) => match never {},
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{instance::AttributeValue, merge_member::MergeMember, written_attribute::WrittenAttribute},
        request_body::RequestBody,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// The fragment's `@context` is the request's, not a member of the
    /// attribute, and a Context Producer's system members are not read.
    #[test]
    fn a_fragment_is_read_without_its_context_and_system_members() {
        let written = WrittenAttribute::from_request_text(
            br#"{"@context": "https://example.org/context.jsonld", "type": "Property", "value": 7, "createdAt": "nonsense"}"#,
            CoreTermAliases::keyword_spellings(),
        )
        .expect("the fragment decodes");
        let member: AttributeValue = written.read(&name("speed"), CoreTermAliases::keyword_spellings()).expect("a Property");
        pretty_assertions::assert_eq!(serde_json::to_string(&member).expect("written"), r#"{"type":"Property","value":7}"#);
    }

    /// The name decides the seals: under `ngsildproof` they are the Property's
    /// own, under any other name they are refused (clause 4.5.2.2).
    #[test]
    fn the_attribute_name_decides_what_a_seal_is() {
        let written = WrittenAttribute::from_request_text(
            br#"{"type": "Property", "value": {"proofValue": "z"}, "entityIdSealed": "urn:ngsi-ld:T:1"}"#,
            CoreTermAliases::keyword_spellings(),
        )
        .expect("the fragment decodes");
        written
            .read::<AttributeValue>(&name("ngsildproof"), CoreTermAliases::keyword_spellings())
            .expect("a proof's seal");
        written
            .read::<AttributeValue>(&name("speed"), CoreTermAliases::keyword_spellings())
            .expect_err("a seal elsewhere is refused");
    }

    /// A partial update reads the fragment as the merge member it is
    /// (clause 5.6.4): an instance may leave its value out.
    #[test]
    fn a_partial_update_reads_an_instance_without_its_value() {
        let written = WrittenAttribute::from_request_text(br#"{"type": "Property", "unitCode": "CEL"}"#, CoreTermAliases::keyword_spellings())
            .expect("the fragment decodes");
        written
            .read::<MergeMember>(&name("temperature"), CoreTermAliases::keyword_spellings())
            .expect("a merge member");
        written
            .read::<AttributeValue>(&name("temperature"), CoreTermAliases::keyword_spellings())
            .expect_err("a whole attribute requires its value");
    }

    /// An empty Attribute Fragment is an RFC 7396 patch naming nothing
    /// (clause 5.4): a partial update reads it as an instance that writes no
    /// member, while a replacement, which writes a whole attribute, refuses it
    /// for the value it lacks (clause 5.2.5).
    #[test]
    fn an_empty_fragment_is_an_instance_writing_nothing() {
        let written = WrittenAttribute::from_request_text(b"{}", CoreTermAliases::keyword_spellings()).expect("the fragment decodes");
        let MergeMember::Single(instance) = written
            .read::<MergeMember>(&name("speed"), CoreTermAliases::keyword_spellings())
            .expect("a merge member")
        else {
            panic!("one instance");
        };
        assert!(
            instance.family.is_none() && instance.value.is_none() && instance.sub_attributes.is_empty(),
            "{instance:?}"
        );
        pretty_assertions::assert_eq!(serde_json::to_string(&instance).expect("written"), "{}");
        written
            .read::<AttributeValue>(&name("speed"), CoreTermAliases::keyword_spellings())
            .expect_err("a whole attribute requires its value");
    }

    /// A fragment that names neither a family nor a value member is the
    /// instance it updates, not the value of a concise Property: its members
    /// are the instance's own.
    #[test]
    fn a_fragment_without_type_or_value_writes_the_members_it_names() {
        let written = WrittenAttribute::from_request_text(br#"{"unitCode": "CEL", "accuracy": 0.5}"#, CoreTermAliases::keyword_spellings())
            .expect("the fragment decodes");
        let MergeMember::Single(instance) = written
            .read::<MergeMember>(&name("temperature"), CoreTermAliases::keyword_spellings())
            .expect("a merge member")
        else {
            panic!("one instance");
        };
        assert!(instance.family.is_none() && instance.value.is_none(), "{instance:?}");
        assert!(instance.unit_code.is_some(), "{instance:?}");
        assert!(instance.sub_attributes.contains_key("accuracy"), "{instance:?}");
    }
}
