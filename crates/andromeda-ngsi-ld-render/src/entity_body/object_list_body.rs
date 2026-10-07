//! A `ListRelationship`'s `objectList`, in the representation it is written in.
//!
//! Table 5.2.37-1: "In the normalized form, each array element holds a JSON
//! object containing a single Attribute with a key called `object` and where
//! the value is a valid URI. In the concise form, each string in the array
//! holds a valid URI." The normalized form, with its clause 4.5.22.2 deletion
//! encoding, is the model's own rule.

use crate::{
    entity_body::reified_member::Reification,
    uri_decoding::{DecodedTarget, UriDecoding, decode_percent_encoded_uri},
};
use andromeda_ngsi_ld_model::attribute::object_list::{NormalizedObjectList, RelationshipObjectList};
use serde::{Serialize, Serializer};

/// One `objectList` or `previousObjectList`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ObjectListBody<'a> {
    /// The ordered targets.
    list: &'a RelationshipObjectList,
    /// The representation the list is written in.
    reification: Reification,
    /// Whether the presentation boundary decodes the targets.
    decoding: UriDecoding,
}

impl<'a> ObjectListBody<'a> {
    /// The list, written in `reification` under `decoding`.
    pub(crate) const fn new(list: &'a RelationshipObjectList, reification: Reification, decoding: UriDecoding) -> ObjectListBody<'a> {
        ObjectListBody { list, reification, decoding }
    }

    /// The first target of the list, written under the list's decoding.
    pub(crate) fn first(self) -> Option<DecodedTarget<'a>> {
        self.list.0.first().map(|target| DecodedTarget::new(target, self.decoding))
    }
}

impl Serialize for ObjectListBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.reification, self.decoding) {
            (Reification::Concise, decoding) => serializer.collect_seq(self.list.iter().map(|target| DecodedTarget::new(target, decoding))),
            (Reification::Normalized, UriDecoding::Keep) => self.list.serialize(serializer),
            (Reification::Normalized, UriDecoding::Decode) => {
                let decoded = self.list.iter().map(|target| decode_percent_encoded_uri(target.as_str())).collect::<Vec<_>>();
                NormalizedObjectList::new(&decoded).serialize(serializer)
            }
        }
    }
}
