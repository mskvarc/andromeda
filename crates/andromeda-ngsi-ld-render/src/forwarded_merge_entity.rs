//! The body of a Merge Entity or a Batch Entity Merge forwarded to a context
//! source, written from its typed form (clauses 4.3.6.8, 5.5.12, 5.6.17 and
//! 5.6.20).
//!
//! Every attribute of the body is a merge member: it changes only the members
//! it names, so an instance may leave out its `type` and its value member, and
//! the source receives it so. Read as a whole Attribute, an instance naming
//! neither would be the bare value of a concise Property (clause 4.5.2.3),
//! which is not what the client wrote. The entity members beside the attributes
//! and the registration's `ngsildConformance` fallback are written as they are
//! for any forwarded entity.

use crate::{
    entity_body::member_walk::MemberWalk,
    forwarded_entity::serialize_entity_members,
    forwarded_fragment::ForwardedFragment,
    version::{adapted_payload::AdaptedPayload, deletion::DeletionEncoding, fallback::VersionFallback},
};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::EntityPatch};
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::NULL_URI;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// One entity of a forwarded merge, whose attributes are merge members.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedMergeEntity<'a> {
    /// The compact partial entity written.
    patch: &'a EntityPatch<MergeMember>,
    /// The version the source conforms to, when the registration names one.
    conformance: Option<SpecVersion>,
    /// What every member is written under.
    walk: MemberWalk<'a>,
}

impl<'a> ForwardedMergeEntity<'a> {
    /// `patch`, written for a source that conforms to `conformance`, when the
    /// registration names a version.
    #[must_use]
    pub const fn new(patch: &'a EntityPatch<MergeMember>, conformance: Option<SpecVersion>) -> ForwardedMergeEntity<'a> {
        let fallback = match conformance {
            Some(version) => VersionFallback::new(version, AdaptedPayload::Entity),
            None => None,
        };
        ForwardedMergeEntity {
            patch,
            conformance,
            walk: MemberWalk::new(None, fallback),
        }
    }
}

impl Serialize for ForwardedMergeEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(None)?;
        serialize_entity_members(&mut object, self.patch, self.walk, None)?;
        for (name, member) in self.patch.attribute_members() {
            match member {
                // The NGSI-LD Null deleting the attribute is written only as
                // the source reads such a deletion, and left out when it reads
                // none.
                MergeMember::NgsiLdNull => match self.walk.attribute_deletion() {
                    Some(DeletionEncoding::NgsiLdNull) => object.serialize_entry(name, NULL_URI.as_str())?,
                    Some(DeletionEncoding::JsonNull) => object.serialize_entry(name, &())?,
                    None => {}
                },
                MergeMember::Single(_) | MergeMember::Multiple(_) => object.serialize_entry(name, &ForwardedFragment::new(member, self.conformance))?,
            }
        }
        object.end()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::forwarded_merge_entity::ForwardedMergeEntity;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::EntityPatch};
    use serde_json::{Value, json};

    /// The merge body `fragment`, read as the fragments they are and written
    /// for a source conforming to `conformance`.
    fn written(fragment: &Value, conformance: Option<SpecVersion>) -> Value {
        let text = JsonLiteral::admit(&fragment.to_string()).expect("a JSON text");
        let patch = EntityPatch::<MergeMember>::from_fragment_node(text.piece()).expect("a merge fragment");
        serde_json::to_value(ForwardedMergeEntity::new(&patch, conformance)).expect("the fragment writes")
    }

    /// Clause 5.5.12: an attribute holding only a temporal member is the
    /// fragment it was written as, not a Property read from its absent value.
    #[test]
    fn an_attribute_holding_only_observed_at_is_written_as_the_fragment_it_is() {
        let fragment = json!({"speed": {"observedAt": "2024-01-01T00:00:00Z"}});
        pretty_assertions::assert_eq!(written(&fragment, None), fragment);
    }

    /// An attribute holding only a sub-attribute is written as it was.
    #[test]
    fn an_attribute_holding_only_a_sub_attribute_is_written_as_the_fragment_it_is() {
        let fragment = json!({"speed": {"accuracy": {"type": "Property", "value": 1}}});
        pretty_assertions::assert_eq!(written(&fragment, None), fragment);
    }

    /// Clause 5.5.12: the NGSI-LD Null removes the attribute, and reaches a
    /// source that reads it.
    #[test]
    fn the_ngsi_ld_null_removes_an_attribute() {
        let fragment = json!({"speed": "urn:ngsi-ld:null"});
        pretty_assertions::assert_eq!(written(&fragment, None), fragment);
    }

    /// The entity members beside the attributes are written with them.
    #[test]
    fn the_entity_members_are_written_beside_the_merge_members() {
        let fragment = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"observedAt": "2024-01-01T00:00:00Z"}});
        pretty_assertions::assert_eq!(written(&fragment, None), fragment);
    }

    /// Table 4.3.6.8-1: a source conforming to 1.0 reads one `type` and no
    /// `observedAt` on a Property, and an instance it keeps nothing of is
    /// written empty.
    #[test]
    fn the_fallback_of_a_1_0_source_is_applied_to_a_merge_member() {
        let fragment = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": ["Vehicle", "Car"], "speed": {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}});
        let version: SpecVersion = "1.0".parse().expect("a version");
        pretty_assertions::assert_eq!(
            written(&fragment, Some(version)),
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}})
        );
    }
}
