//! The canonical encoding of the pieces of an expanded document: one expanded
//! value, one language map, one written instance. Each is written exactly as it
//! is written inside the whole document, so a store keeping a piece in a column
//! of its own keeps the same JSON the document would have held.

use crate::{
    codec::{carrier_encode::LanguageEntriesOut, document_encode::InstanceDocumentOut, value_encode::ValueOut},
    document::instance_document::InstanceDocument,
    expanded_value::ExpandedValue,
    language_entries::LanguageEntries,
    value_mode::{Fragment, Stored},
};
use serde::{Serialize, Serializer};

impl Serialize for ExpandedValue<Stored> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ValueOut(self).serialize(serializer)
    }
}

impl Serialize for ExpandedValue<Fragment> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ValueOut(self).serialize(serializer)
    }
}

impl Serialize for LanguageEntries<Stored> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LanguageEntriesOut(self).serialize(serializer)
    }
}

impl Serialize for LanguageEntries<Fragment> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LanguageEntriesOut(self).serialize(serializer)
    }
}

impl Serialize for InstanceDocument {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        InstanceDocumentOut(self).serialize(serializer)
    }
}
