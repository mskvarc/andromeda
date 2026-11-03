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
    request_body::{DecodeBounds, RequestBody, read_as_written},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, bounded_piece::BoundedPiece, context_reading::ContextReading, produced_node::ProducedNode},
    value_budget::ValueBudget,
};
use serde::Deserialize;
use std::convert::Infallible;

/// A bare Attribute Fragment as a request wrote it, its `@context` left out.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct WrittenAttribute {
    /// The fragment as the JSON text it was written as.
    literal: JsonLiteral,
    /// The bounds the fragment is read within, when the stack published them.
    #[serde(skip)]
    bounds: Option<DecodeBounds>,
}

impl RequestBody for WrittenAttribute {
    /// Hold the fragment as written, its top-level `@context` left out: which
    /// members it has depends on the attribute it is read for, so the reading
    /// of its context is applied once, by [`WrittenAttribute::read`].
    fn from_request_text(text: &[u8], _reading: ContextReading<'_>) -> Result<WrittenAttribute, serde_json::Error> {
        read_as_written(text)
    }

    /// Hold the fragment as [`RequestBody::from_request_text`] does and keep
    /// the bounds it is read within once the attribute name is known.
    fn from_bounded_request_text(text: &[u8], reading: ContextReading<'_>, bounds: DecodeBounds) -> Result<WrittenAttribute, serde_json::Error> {
        let mut written = WrittenAttribute::from_request_text(text, reading)?;
        written.bounds = Some(bounds);
        Ok(written)
    }
}

impl WrittenAttribute {
    /// The member the fragment is, written for the attribute `attribute`, as a
    /// Context Producer wrote it under a context read as `reading` reads it:
    /// each member is read as the core term it stands for and without the
    /// container of its term, and the system members of clause 5.2.2 it carries
    /// are not read. A fragment decoded within bounds is read within them: a
    /// geometry and a growing member are recorded no further than the bounds
    /// let an entity hold (clause 6.3.2).
    ///
    /// # Errors
    ///
    /// Returns the [`RefusedNode`] of a fragment no clause admits where an
    /// attribute stands, located from the fragment.
    pub fn read<M: MemberReading>(&self, attribute: &AttributeName, reading: ContextReading<'_>) -> Result<M, RefusedNode> {
        match self.bounds {
            Some(bounds) => {
                let values = ValueBudget::new(bounds.entity_values());
                read_node(
                    attribute,
                    BoundedPiece::new(self.literal.piece(), bounds.geometry_positions(), &values),
                    reading,
                )
            }
            None => read_node(attribute, self.literal.piece(), reading),
        }
    }
}

/// The member `node` is for the attribute `attribute`, read through `reading`.
fn read_node<'n, M: MemberReading, N: CompactNode<'n, Unreadable = Infallible>>(
    attribute: &AttributeName,
    node: N,
    reading: ContextReading<'n>,
) -> Result<M, RefusedNode> {
    match M::from_fragment_node(attribute.as_str(), ProducedNode::new(reading.read(node))) {
        Ok(member) => Ok(member),
        Err(NodeReadError::Refused(refused)) => Err(refused),
        Err(NodeReadError::Unreadable(never)) => match never {},
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute::{
            instance::{Attribute, AttributeValue},
            merge_member::MergeMember,
            written_attribute::WrittenAttribute,
        },
        request_body::{DecodeBounds, RequestBody},
    };
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use std::num::NonZeroUsize;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// The fragment's `@context` is the request's, not a member of the
    /// attribute, and a Context Producer's system members are not read.
    #[test]
    fn a_fragment_is_read_without_its_context_and_system_members() {
        let written = WrittenAttribute::from_request_text(
            br#"{"@context": "https://example.org/context.jsonld", "type": "Property", "value": 7, "createdAt": "nonsense"}"#,
            ContextReading::keyword_spellings(),
        )
        .expect("the fragment decodes");
        let member: AttributeValue = written.read(&name("speed"), ContextReading::keyword_spellings()).expect("a Property");
        pretty_assertions::assert_eq!(serde_json::to_string(&member).expect("written"), r#"{"type":"Property","value":7}"#);
    }

    /// The name decides the seals: under `ngsildproof` they are the Property's
    /// own, under any other name they are refused (clause 4.5.2.2).
    #[test]
    fn the_attribute_name_decides_what_a_seal_is() {
        let written = WrittenAttribute::from_request_text(
            br#"{"type": "Property", "value": {"proofValue": "z"}, "entityIdSealed": "urn:ngsi-ld:T:1"}"#,
            ContextReading::keyword_spellings(),
        )
        .expect("the fragment decodes");
        written
            .read::<AttributeValue>(&name("ngsildproof"), ContextReading::keyword_spellings())
            .expect("a proof's seal");
        written
            .read::<AttributeValue>(&name("speed"), ContextReading::keyword_spellings())
            .expect_err("a seal elsewhere is refused");
    }

    /// A partial update reads the fragment as the merge member it is
    /// (clause 5.6.4): an instance may leave its value out.
    #[test]
    fn a_partial_update_reads_an_instance_without_its_value() {
        let written = WrittenAttribute::from_request_text(br#"{"type": "Property", "unitCode": "CEL"}"#, ContextReading::keyword_spellings())
            .expect("the fragment decodes");
        written
            .read::<MergeMember>(&name("temperature"), ContextReading::keyword_spellings())
            .expect("a merge member");
        written
            .read::<AttributeValue>(&name("temperature"), ContextReading::keyword_spellings())
            .expect_err("a whole attribute requires its value");
    }

    /// An empty Attribute Fragment is an RFC 7396 patch naming nothing
    /// (clause 5.4): a partial update reads it as an instance that writes no
    /// member, while a replacement, which writes a whole attribute, refuses it
    /// for the value it lacks (clause 5.2.5).
    #[test]
    fn an_empty_fragment_is_an_instance_writing_nothing() {
        let written = WrittenAttribute::from_request_text(b"{}", ContextReading::keyword_spellings()).expect("the fragment decodes");
        let MergeMember::Single(instance) = written
            .read::<MergeMember>(&name("speed"), ContextReading::keyword_spellings())
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
            .read::<AttributeValue>(&name("speed"), ContextReading::keyword_spellings())
            .expect_err("a whole attribute requires its value");
    }

    /// A fragment that names neither a family nor a value member is the
    /// instance it updates, not the value of a concise Property: its members
    /// are the instance's own.
    #[test]
    fn a_fragment_without_type_or_value_writes_the_members_it_names() {
        let written =
            WrittenAttribute::from_request_text(br#"{"unitCode": "CEL", "accuracy": 0.5}"#, ContextReading::keyword_spellings()).expect("the fragment decodes");
        let MergeMember::Single(instance) = written
            .read::<MergeMember>(&name("temperature"), ContextReading::keyword_spellings())
            .expect("a merge member")
        else {
            panic!("one instance");
        };
        assert!(instance.family.is_none() && instance.value.is_none(), "{instance:?}");
        assert!(instance.unit_code.is_some(), "{instance:?}");
        assert!(instance.sub_attributes.contains_key("accuracy"), "{instance:?}");
    }

    fn bounded(text: &str, positions: usize, entity_values: usize) -> WrittenAttribute {
        let bounds = DecodeBounds::new(
            GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit")),
            JsonValueLimit::new(NonZeroUsize::new(entity_values).expect("a positive limit")),
        );
        WrittenAttribute::from_bounded_request_text(text.as_bytes(), ContextReading::keyword_spellings(), bounds).expect("the fragment decodes")
    }

    /// Clause 6.3.2: a geometry past the position limit is recorded no further
    /// than the limit, so writing it back is refused.
    #[test]
    fn a_fragment_geometry_past_the_position_limit_is_not_recorded_whole() {
        let text = r#"{"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2], [3, 3]]}}"#;
        let within: AttributeValue = bounded(text, 4, 1_000)
            .read(&name("location"), ContextReading::keyword_spellings())
            .expect("a GeoProperty");
        serde_json::to_string(&within).expect("a geometry within the limit");
        let past: AttributeValue = bounded(text, 1, 1_000)
            .read(&name("location"), ContextReading::keyword_spellings())
            .expect("a GeoProperty");
        serde_json::to_string(&past).expect_err("a geometry past the limit is not recorded");
    }

    #[test]
    fn the_sub_attributes_of_a_fragment_past_the_value_limit_are_cut() {
        let sub_attributes = (0..80)
            .map(|index| format!(r#""s{index}": {{"type": "Property", "value": 1}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!(r#"{{"type": "Property", "value": 1, {sub_attributes}}}"#);
        let held = |entity_values: usize| match bounded(&text, 1_000, entity_values)
            .read::<AttributeValue>(&name("speed"), ContextReading::keyword_spellings())
            .expect("a Property")
        {
            AttributeValue::Single(attribute) => match *attribute {
                Attribute::Property(property) => property.common.sub_attributes.len(),
                other @ (Attribute::Relationship(_)
                | Attribute::VocabProperty(_)
                | Attribute::GeoProperty(_)
                | Attribute::LanguageProperty(_)
                | Attribute::JsonProperty(_)
                | Attribute::ListProperty(_)
                | Attribute::ListRelationship(_)) => panic!("not a Property: {other:?}"),
            },
            other @ (AttributeValue::NgsiLdNull | AttributeValue::Multiple(_)) => panic!("not one instance: {other:?}"),
        };
        assert!(held(10) < 80, "{}", held(10));
        pretty_assertions::assert_eq!(held(10_000), 80);
    }
}
