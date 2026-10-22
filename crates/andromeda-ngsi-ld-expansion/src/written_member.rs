//! The attribute members of the typed compact model the typed walk expands.

use andromeda_ngsi_ld_model::attribute::{instance::AttributeValue, merge_member::MergeMember};
use serde::Serialize;

/// One attribute member the typed walk expands.
#[derive(Debug, Clone, Copy)]
pub enum WrittenMemberRef<'a> {
    /// A member of an entity.
    Attribute(&'a AttributeValue),
    /// A member of a merge fragment (clause 5.5.12).
    Merge(&'a MergeMember),
}

impl WrittenMemberRef<'_> {
    /// Whether the member is the NGSI-LD Null written in place of the whole
    /// member.
    #[must_use]
    pub const fn is_ngsi_ld_null(self) -> bool {
        matches!(
            self,
            WrittenMemberRef::Attribute(AttributeValue::NgsiLdNull) | WrittenMemberRef::Merge(MergeMember::NgsiLdNull)
        )
    }
}

/// An attribute member the typed walk expands: a member of an entity, or a
/// member of a merge fragment.
///
/// The member serializes to the compact JSON the processor expands when the
/// walk declines it.
pub trait WrittenMember: Serialize + Sync {
    /// The member, as the walk reads it.
    fn written(&self) -> WrittenMemberRef<'_>;
}

impl WrittenMember for AttributeValue {
    fn written(&self) -> WrittenMemberRef<'_> {
        WrittenMemberRef::Attribute(self)
    }
}

impl WrittenMember for MergeMember {
    fn written(&self) -> WrittenMemberRef<'_> {
        WrittenMemberRef::Merge(self)
    }
}
