//! How a value tree of either mode is read by the `q` evaluator.
//!
//! A state value tree and a written fragment's value tree differ only in that
//! the fragment may carry the NGSI-LD Null where a structured member or a
//! language entry stands (clause 5.5.12). A deletion holds no value clause 4.9
//! could compare, so it reads as nothing.

use crate::{
    expanded_value::ExpandedValue,
    value_mode::{Fragment, FragmentMember, FragmentText, Stored, ValueMode},
};

/// The reads the `q` evaluator takes from one value mode.
pub trait QueriedMode: ValueMode {
    /// The values one member of a structured value holds; none for a deleted
    /// member.
    fn member_values(member: &Self::Member) -> &[ExpandedValue<Self>];

    /// The text one language-map entry holds; `None` for a deleted language.
    fn entry_text(text: &Self::Text) -> Option<&str>;
}

impl QueriedMode for Stored {
    fn member_values(member: &Self::Member) -> &[ExpandedValue<Stored>] {
        member
    }

    fn entry_text(text: &Self::Text) -> Option<&str> {
        Some(text.as_str())
    }
}

impl QueriedMode for Fragment {
    fn member_values(member: &FragmentMember) -> &[ExpandedValue<Fragment>] {
        match member {
            FragmentMember::Values(values) => values,
            FragmentMember::Remove => &[],
        }
    }

    fn entry_text(text: &FragmentText) -> Option<&str> {
        match text {
            FragmentText::Text(text) => Some(text.as_str()),
            FragmentText::Remove => None,
        }
    }
}
