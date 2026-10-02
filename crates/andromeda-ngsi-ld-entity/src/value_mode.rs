//! Whether a value tree is entity state or a written fragment.
//!
//! The NGSI-LD Null (clause 4.5.0) is a deletion instruction. A merge fragment
//! (clause 5.5.12) may carry it inside a structured Property value, where it
//! deletes the member it stands on, and inside a language map, where it
//! deletes one language; entity state never holds it. The value types are
//! therefore parameterised by a mode, and only the fragment mode gives the
//! deletion a place to live: a state value tree cannot express it.

use crate::expanded_value::ExpandedValue;
use compact_str::CompactString;
use std::fmt::Debug;

/// The places a value tree of one mode can hold.
pub trait ValueMode: Debug + Clone + PartialEq + private::Sealed {
    /// The values one member of a structured value holds.
    type Member: Debug + Clone + PartialEq;
    /// The text one language-map entry holds.
    type Text: Debug + Clone + PartialEq;
}

/// Entity state: no deletion anywhere.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stored {}

impl ValueMode for Stored {
    type Member = Vec<ExpandedValue<Stored>>;
    type Text = CompactString;
}

/// A written fragment: a structured member or a language entry may be the
/// NGSI-LD Null.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fragment {}

impl ValueMode for Fragment {
    type Member = FragmentMember;
    type Text = FragmentText;
}

/// One member of a structured value in a written fragment.
#[derive(Clone, Debug, PartialEq)]
pub enum FragmentMember {
    /// The member's values.
    Values(Vec<ExpandedValue<Fragment>>),
    /// The NGSI-LD Null: the merge deletes the member (clause 5.5.12).
    Remove,
}

/// The text of one language-map entry in a written fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FragmentText {
    /// The text in that language.
    Text(CompactString),
    /// The NGSI-LD Null: the merge deletes that language (clause 5.5.12).
    Remove,
}

/// Keeps the two modes the only ones.
mod private {
    use crate::value_mode::{Fragment, Stored};

    /// Implemented by the two modes alone.
    pub trait Sealed {}

    impl Sealed for Stored {}
    impl Sealed for Fragment {}
}
