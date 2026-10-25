//! How the attributes of a compacted partial entity are read.
//!
//! A partial entity forwarded for most writes holds attribute members as an
//! entity holds them, where clause 4.5.2.3 reads an object naming neither a
//! `type` nor a value member as the value of a concise Property. The one
//! attribute of a Partial Attribute Update or a Replace Attribute is instead
//! the bare Attribute Fragment of clauses 5.6.4 and 5.6.19, which is the
//! instance it updates whatever it names.

use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::EntityPatch, node_refusal::NodeReadError};
use andromeda_ngsi_ld_representation::compact_node::CompactNode;

/// How the attributes of a compacted partial entity are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchReading {
    /// As members written where an attribute stands.
    Members,
    /// As bare Attribute Fragments.
    Fragments,
}

impl PatchReading {
    /// The partial entity `node` is, its attributes read this way.
    pub(crate) fn read<'n, M: MemberReading, N: CompactNode<'n>>(self, node: N) -> Result<EntityPatch<M>, NodeReadError<N::Unreadable>> {
        match self {
            PatchReading::Members => EntityPatch::from_node(node),
            PatchReading::Fragments => EntityPatch::from_fragment_node(node),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::patch_reading::PatchReading;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::EntityPatch};

    fn read(reading: PatchReading, text: &str) -> serde_json::Value {
        let written = JsonLiteral::admit(text).expect("a JSON text");
        let patch: EntityPatch<MergeMember> = reading.read(written.piece()).expect("a partial entity");
        serde_json::to_value(&patch).expect("the patch writes")
    }

    /// Clause 4.5.2.3: in place of a member, an object naming neither a `type`
    /// nor a value member is the value of a concise Property.
    #[test]
    fn a_member_naming_no_family_is_a_concise_property_value() {
        pretty_assertions::assert_eq!(
            read(PatchReading::Members, r#"{"speed": {"source": {"type": "Property", "value": "radar"}}}"#),
            serde_json::json!({"speed": {"type": "Property", "value": {"source": {"type": "Property", "value": "radar"}}}})
        );
    }

    /// Clause 5.6.4: the bare Attribute Fragment is the instance it updates,
    /// so the object's members stay the instance's own.
    #[test]
    fn a_fragment_naming_no_family_keeps_its_members() {
        pretty_assertions::assert_eq!(
            read(PatchReading::Fragments, r#"{"speed": {"source": {"type": "Property", "value": "radar"}}}"#),
            serde_json::json!({"speed": {"source": {"type": "Property", "value": "radar"}}})
        );
    }
}
