//! A `ListRelationship`'s `objectList`, in the representation it is written in.
//!
//! Table 5.2.37-1: "In the normalized form, each array element holds a JSON
//! object containing a single Attribute with a key called `object` and where
//! the value is a valid URI. In the concise form, each string in the array
//! holds a valid URI." The normalized form, with its clause 4.5.22.2 deletion
//! encoding, is the model's own rule. Every target is written as it was
//! stored: RFC 3986 section 6.2.2.2 does not make a percent-encoded reserved
//! character equivalent to the character itself.

use crate::entity_body::reified_member::Reification;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::attribute::object_list::RelationshipObjectList;
use serde::{Serialize, Serializer};

/// One `objectList` or `previousObjectList`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ObjectListBody<'a> {
    /// The ordered targets.
    list: &'a RelationshipObjectList,
    /// The representation the list is written in.
    reification: Reification,
}

impl<'a> ObjectListBody<'a> {
    /// The list, written in `reification`.
    pub(crate) const fn new(list: &'a RelationshipObjectList, reification: Reification) -> ObjectListBody<'a> {
        ObjectListBody { list, reification }
    }

    /// The first target of the list.
    pub(crate) fn first(self) -> Option<&'a Uri> {
        self.list.0.first()
    }
}

impl Serialize for ObjectListBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.reification {
            Reification::Concise => serializer.collect_seq(self.list.iter()),
            Reification::Normalized => self.list.serialize(serializer),
        }
    }
}
