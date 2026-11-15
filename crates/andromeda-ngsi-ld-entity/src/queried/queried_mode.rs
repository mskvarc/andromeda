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
    /// The values one member of a structured value holds, none for a member
    /// holding the empty array, and `None` for a member a fragment deletes,
    /// which is no member at all.
    fn standing_values(member: &Self::Member) -> Option<&[ExpandedValue<Self>]>;

    /// The text one language-map entry holds; `None` for a deleted language.
    fn entry_text(text: &Self::Text) -> Option<&str>;
}

impl QueriedMode for Stored {
    fn standing_values(member: &Vec<ExpandedValue<Stored>>) -> Option<&[ExpandedValue<Stored>]> {
        Some(member)
    }

    fn entry_text(text: &Self::Text) -> Option<&str> {
        Some(text.as_str())
    }
}

impl QueriedMode for Fragment {
    fn standing_values(member: &FragmentMember) -> Option<&[ExpandedValue<Fragment>]> {
        match member {
            FragmentMember::Values(values) => Some(values),
            FragmentMember::Remove => None,
        }
    }

    fn entry_text(text: &FragmentText) -> Option<&str> {
        match text {
            FragmentText::Text(text) => Some(text.as_str()),
            FragmentText::Remove => None,
        }
    }
}
