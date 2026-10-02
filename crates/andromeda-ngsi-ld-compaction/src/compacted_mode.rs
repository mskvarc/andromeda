//! What the walk reads from the members and texts of each value mode.
//!
//! Entity state holds plain values; a written fragment may hold the NGSI-LD
//! Null where a merge deletes a structured member or one language (clause
//! 5.5.12). The canonical codec writes that Null as the value object
//! `{"@value": "urn:ngsi-ld:null"}` and as the text `urn:ngsi-ld:null`, and
//! compaction reads exactly what the codec writes.

use andromeda_ngsi_ld_entity::{
    expanded_value::ExpandedValue,
    value_mode::{Fragment, FragmentMember, FragmentText, Stored, ValueMode},
};
use andromeda_ngsi_ld_vocabulary::NULL_URI;

/// One structured member, as the walk reads it.
pub(crate) enum MemberValues<'a, M: ValueMode> {
    /// The member's values.
    Values(&'a [ExpandedValue<M>]),
    /// The NGSI-LD Null standing for the member.
    Null,
}

/// A value mode the walk compacts.
pub(crate) trait CompactedMode: ValueMode {
    /// The values one structured member holds.
    fn member_values(member: &Self::Member) -> MemberValues<'_, Self>;

    /// The text one language entry holds.
    fn text(text: &Self::Text) -> &str;
}

impl CompactedMode for Stored {
    fn member_values(member: &Vec<ExpandedValue<Stored>>) -> MemberValues<'_, Stored> {
        MemberValues::Values(member)
    }

    fn text(text: &Self::Text) -> &str {
        text
    }
}

impl CompactedMode for Fragment {
    fn member_values(member: &FragmentMember) -> MemberValues<'_, Fragment> {
        match member {
            FragmentMember::Values(values) => MemberValues::Values(values),
            FragmentMember::Remove => MemberValues::Null,
        }
    }

    fn text(text: &FragmentText) -> &str {
        match text {
            FragmentText::Text(text) => text,
            FragmentText::Remove => NULL_URI.as_str(),
        }
    }
}
