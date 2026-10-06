//! The items each value carrier of an attribute instance holds (clause 4.5).
//!
//! A carrier holds its value in the expanded shape the canonical codec writes:
//! `hasObject`, `hasVocab` and `objectType` as node references, `hasLanguageMap`
//! as one value object per entry, `hasJSON` as `@json` literals,
//! `hasValueList` and `hasObjectList` as one list object, a `GeoProperty`'s
//! `hasValue` as its geometry node or the clause 4.7.2 string. A stored and a
//! written instance carry the same shapes, so both build their items here.

use crate::{compacted_mode::CompactedMode, expanded_item::ExpandedItem, undecidable::Undecidable};
use andromeda_json_literal::{JsonLiteral, processor_json::processor_string};
use andromeda_ngsi_ld_entity::{
    document::value_carrier::ValueCarrier,
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    language_entries::LanguageEntries,
    list_object::ListObject,
};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use jsonld::Value;

/// One node reference per IRI.
pub(crate) fn references<'a, 'i, M: CompactedMode>(iris: impl IntoIterator<Item = &'i str>) -> Vec<ExpandedItem<'a, M>> {
    iris.into_iter().map(ExpandedItem::reference).collect()
}

/// One value object per language entry, tagged unless it is the `@none`
/// entry.
pub(crate) fn language_entries<'a, M: CompactedMode>(entries: &LanguageEntries<M>) -> Vec<ExpandedItem<'a, M>> {
    entries
        .entries()
        .iter()
        .map(|entry| {
            let text = M::text(entry.text());
            match entry.language() {
                Some(language) => ExpandedItem::tagged_text(text, language.as_str()),
                None => ExpandedItem::text(text),
            }
        })
        .collect()
}

/// One `@json` value object per literal.
///
/// # Errors
///
/// As [`ExpandedItem::json`].
pub(crate) fn json_literals<'a, M: CompactedMode>(literals: &[JsonLiteral]) -> Result<Vec<ExpandedItem<'a, M>>, Undecidable> {
    literals.iter().map(ExpandedItem::json).collect()
}

/// The one list object of a `ListRelationship`, each item in its stored
/// spelling.
pub(crate) fn object_list<M: CompactedMode>(objects: &[ListObject]) -> Vec<ExpandedItem<'_, M>> {
    let items = objects
        .iter()
        .map(|object| match object {
            ListObject::Bare(uri) => ExpandedItem::reference(uri.as_str()),
            ListObject::Literal(uri) => ExpandedItem::text(uri.as_str()),
            ListObject::Wrapped(uri) => ExpandedItem::WrappedObject(uri),
        })
        .collect();
    vec![ExpandedItem::List(items)]
}

/// The one list object of a `ListProperty`.
pub(crate) fn value_list<M: CompactedMode>(items: &[ExpandedValue<M>]) -> Result<Vec<ExpandedItem<'_, M>>, Undecidable> {
    ExpandedItem::of_values(items).map(|items| vec![ExpandedItem::List(items)])
}

/// The value of a `GeoProperty`: its geometry node, or the clause 4.7.2
/// string it was written as.
pub(crate) fn geometry<M: CompactedMode>(geometry: &GeoValue) -> Vec<ExpandedItem<'_, M>> {
    vec![match geometry {
        GeoValue::Geometry(value) => ExpandedItem::Geometry(value),
        GeoValue::Encoded(encoded) => ExpandedItem::text(encoded.as_str()),
    }]
}

/// The deletion encoding of `carrier` (clause 4.5.0), as the canonical codec
/// writes it: a reference to the NGSI-LD Null for the reference carriers, the
/// Null as a `@json` literal for `json`, a list of the Null for the list
/// families, and the Null as a value object otherwise.
pub(crate) fn deletion<'a, M: CompactedMode>(carrier: ValueCarrier) -> Vec<ExpandedItem<'a, M>> {
    let null = NULL_URI.as_str();
    vec![match carrier {
        ValueCarrier::Object | ValueCarrier::Vocab => ExpandedItem::reference(null),
        ValueCarrier::Json => ExpandedItem::Value(Value::Json(processor_string(null))),
        ValueCarrier::ValueList | ValueCarrier::ObjectList => ExpandedItem::List(vec![ExpandedItem::text(null)]),
        ValueCarrier::Value | ValueCarrier::LanguageMap => ExpandedItem::text(null),
    }]
}
