//! Writing the value carriers and the reserved members of an instance in their
//! expanded shapes.
//!
//! The pieces here are shared by stored instances and written ones: a value is
//! written the same way whether it is state or part of a fragment, and the
//! NGSI-LD Null of a fragment is written in the deletion encoding clause 4.5.0
//! gives its member.

use crate::{
    codec::value_encode::{IdOut, JsonLiteralOut, ListOut, ModeEncoding, NullJsonOut, NullValueOut, TextOut, TypedTextOut},
    document::{slot::Slot, value_carrier::ValueCarrier},
    language_entries::{LanguageEntries, LanguageEntry},
    list_object::ListObject,
    value_mode::ValueMode,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, uri::Uri};
use andromeda_ngsi_ld_vocabulary::{
    expanded::{classes, properties},
    ngsi_ld_null::NULL_URI,
};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::iter::once;

/// `@id` references to URIs.
pub(crate) struct UrisOut<'a>(pub(crate) &'a [Uri]);

impl Serialize for UrisOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|uri| IdOut(uri.as_str())))
    }
}

/// `@id` references to names: references, or entity type names.
pub(crate) struct NamesOut<'a, N>(pub(crate) &'a [N]);

impl<N: AsRef<str>> Serialize for NamesOut<'_, N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|name| IdOut(name.as_ref())))
    }
}

/// JSON literals.
pub(crate) struct JsonLiteralsOut<'a>(pub(crate) &'a [JsonLiteral]);

impl Serialize for JsonLiteralsOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(JsonLiteralOut))
    }
}

/// The entries of a language map.
pub(crate) struct LanguageEntriesOut<'a, M: ValueMode>(pub(crate) &'a LanguageEntries<M>);

impl<M: ModeEncoding> Serialize for LanguageEntriesOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.entries().iter().map(LanguageEntryOut))
    }
}

/// One language entry: `{"@value": text, "@language": tag}`, without the
/// language for the `@none` entry.
struct LanguageEntryOut<'a, M: ValueMode>(&'a LanguageEntry<M>);

impl<M: ModeEncoding> Serialize for LanguageEntryOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@value", M::text(self.0.text()))?;
        if let Some(language) = self.0.language() {
            map.serialize_entry("@language", language.as_str())?;
        }
        map.end()
    }
}

/// The items of an object list, each in its stored spelling.
pub(crate) struct ListObjectsOut<'a>(pub(crate) &'a [ListObject]);

impl Serialize for ListObjectsOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(ListObjectOut))
    }
}

/// One object-list item.
struct ListObjectOut<'a>(&'a ListObject);

impl Serialize for ListObjectOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ListObject::Bare(uri) => IdOut(uri.as_str()).serialize(serializer),
            ListObject::Literal(uri) => TextOut(uri.as_str()).serialize(serializer),
            ListObject::Wrapped(uri) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(properties::OBJECT.as_str(), &[IdOut(uri.as_str())])?;
                map.end()
            }
        }
    }
}

/// A one-element array holding `T`.
pub(crate) struct One<T>(pub(crate) T);

impl<T: Serialize> Serialize for One<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(once(&self.0))
    }
}

/// Write `"@type": ["<family IRI>"]`.
pub(crate) fn write_family<M: SerializeMap>(map: &mut M, family_iri: &str) -> Result<(), M::Error> {
    map.serialize_entry("@type", &One(family_iri))
}

/// Write a `DateTime`-typed member, set or deleted.
pub(crate) fn write_date_time<M: SerializeMap>(map: &mut M, key: &str, value: Option<&Slot<NgsiLdDateTime>>) -> Result<(), M::Error> {
    match value {
        Some(Slot::Set(instant)) => write_date_time_text(map, key, &instant.to_string()),
        Some(Slot::Remove) => write_date_time_text(map, key, NULL_URI.as_str()),
        None => Ok(()),
    }
}

/// Write `"<key>": [{"@type": DateTime, "@value": text}]`.
pub(crate) fn write_date_time_text<M: SerializeMap>(map: &mut M, key: &str, text: &str) -> Result<(), M::Error> {
    map.serialize_entry(
        key,
        &One(TypedTextOut {
            datatype: classes::DATE_TIME.as_str(),
            text,
        }),
    )
}

/// Write `"<key>": [{"@id": iri}]`.
pub(crate) fn write_reference<M: SerializeMap>(map: &mut M, key: &str, iri: &str) -> Result<(), M::Error> {
    map.serialize_entry(key, &One(IdOut(iri)))
}

/// Write `"<key>": [{"@value": text}]`.
pub(crate) fn write_text<M: SerializeMap>(map: &mut M, key: &str, text: &str) -> Result<(), M::Error> {
    map.serialize_entry(key, &One(TextOut(text)))
}

/// Write the deletion encoding of `carrier` (clause 4.5.0): `@id` for the
/// reference carriers, a `@json` literal for `json`, a list of the Null for the
/// list families, and a value object otherwise.
pub(crate) fn write_carrier_deletion<M: SerializeMap>(map: &mut M, carrier: ValueCarrier) -> Result<(), M::Error> {
    let key = carrier.iri().into_inner();
    match carrier {
        ValueCarrier::Object | ValueCarrier::Vocab => write_reference(map, key, NULL_URI.as_str()),
        ValueCarrier::Json => map.serialize_entry(key, &One(NullJsonOut)),
        ValueCarrier::ValueList | ValueCarrier::ObjectList => map.serialize_entry(key, &One(ListOut(One(NullValueOut)))),
        ValueCarrier::Value | ValueCarrier::LanguageMap => map.serialize_entry(key, &One(NullValueOut)),
    }
}
