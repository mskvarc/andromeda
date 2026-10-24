//! The values an attribute instance's family member holds, in order, as the
//! `q` evaluator reads them (clause 4.9).
//!
//! Each family keeps its values in a collection of its own type, so the
//! iterator over them is one enum over the per-family slice iterators: no
//! value is copied and nothing is boxed.

use crate::{
    document::written_value::WrittenValue,
    expanded_value::{ExpandedValue, ValueNode},
    family_value::FamilyValue,
    geo_value::GeoValue,
    language_entries::LanguageEntry,
    list_object::ListObject,
    queried::{queried_mode::QueriedMode, value_node_reading::item_of},
    value_mode::{Fragment, Stored},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_query::{
    filter::{language_matching::LanguageMatching, target_language::LanguageSelector},
    queried_entity::value_item::{LiteralValue, ValueItem},
};
use std::{borrow::Cow, slice::Iter};

/// The values of one family member, in the order the instance holds them.
#[derive(Debug)]
pub enum FamilyValues<'a, M: QueriedMode> {
    /// Expanded values: a Property's `value`, a `ListProperty`'s items, with
    /// the `valueType` that declares their datatype.
    Expanded(Iter<'a, ExpandedValue<M>>, Option<&'a NgsiLdReference>),
    /// A Relationship's objects.
    Objects(Iter<'a, Uri>),
    /// A `VocabProperty`'s terms.
    Terms(Iter<'a, NgsiLdReference>),
    /// A `LanguageProperty`'s entries; every language's string is a value
    /// (clause 4.9 with no target language).
    Entries(Iter<'a, LanguageEntry<M>>),
    /// A `JsonProperty`'s JSON literals.
    Json(Iter<'a, JsonLiteral>),
    /// A `ListRelationship`'s objects, whichever spelling each was stored in.
    ListObjects(Iter<'a, ListObject>),
    /// A `GeoProperty`'s one geometry, or nothing at all.
    Single(Option<ValueItem<'a, &'a ValueNode<M>>>),
}

impl<'a> FamilyValues<'a, Stored> {
    /// The values a state instance's family member holds.
    #[must_use]
    pub fn of_state(value: &'a FamilyValue) -> FamilyValues<'a, Stored> {
        match value {
            FamilyValue::Property { values, value_type, .. } => FamilyValues::Expanded(values.iter(), value_type.as_ref()),
            FamilyValue::GeoProperty { geometry, .. } => FamilyValues::Single(Some(geometry_item(geometry))),
            FamilyValue::Relationship { objects, .. } => FamilyValues::Objects(objects.iter()),
            FamilyValue::LanguageProperty { entries, .. } => FamilyValues::Entries(entries.entries().iter()),
            FamilyValue::VocabProperty { vocab, .. } => FamilyValues::Terms(vocab.iter()),
            FamilyValue::JsonProperty { json, .. } => FamilyValues::Json(json.iter()),
            FamilyValue::ListProperty { items, value_type, .. } => FamilyValues::Expanded(items.iter(), value_type.as_ref()),
            FamilyValue::ListRelationship { objects, .. } => FamilyValues::ListObjects(objects.iter()),
        }
    }
}

impl<'a> FamilyValues<'a, Fragment> {
    /// The values a written instance's value member holds, beside the
    /// `valueType` the instance writes.
    #[must_use]
    pub fn of_written(value: &'a WrittenValue, value_type: Option<&'a NgsiLdReference>) -> FamilyValues<'a, Fragment> {
        match value {
            WrittenValue::Value(values) => FamilyValues::Expanded(values.iter(), value_type),
            WrittenValue::Geometry(geometry) => FamilyValues::Single(Some(geometry_item(geometry))),
            WrittenValue::Object(objects) => FamilyValues::Objects(objects.iter()),
            WrittenValue::LanguageMap(entries) => FamilyValues::Entries(entries.entries().iter()),
            WrittenValue::Vocab(vocab) => FamilyValues::Terms(vocab.iter()),
            WrittenValue::Json(json) => FamilyValues::Json(json.iter()),
            WrittenValue::ValueList(items) => FamilyValues::Expanded(items.iter(), value_type),
            WrittenValue::ObjectList(objects) => FamilyValues::ListObjects(objects.iter()),
        }
    }

    /// No value: an instance whose value member is absent or deleted.
    #[must_use]
    pub const fn none() -> FamilyValues<'a, Fragment> {
        FamilyValues::Single(None)
    }
}

/// The strings a language map holds under every tag `wanted` admits under
/// `matching`, in order: the clause 4.9 target value of a `LanguageProperty`
/// with a target language.
///
/// The `@none` entry carries no tag and so is no language's string, and a
/// deleted language holds no string.
pub fn language_strings<'a, 'l, M: QueriedMode>(
    entries: &'a [LanguageEntry<M>],
    wanted: LanguageSelector<'l>,
    matching: LanguageMatching,
) -> impl Iterator<Item = ValueItem<'a, &'a ValueNode<M>>> + use<'a, 'l, M> {
    entries
        .iter()
        .filter(move |entry| entry.language().is_some_and(|tag| wanted.admits(tag, matching)))
        .filter_map(|entry| M::entry_text(entry.text()))
        .map(|text| ValueItem::Literal(LiteralValue::Text(Cow::Borrowed(text))))
}

/// A `GeoProperty`'s value: a geometry carries no literal and no member a path
/// can address, while the clause 4.7.2 string form is a String like any other.
fn geometry_item<M: QueriedMode>(geometry: &GeoValue) -> ValueItem<'_, &ValueNode<M>> {
    match geometry {
        GeoValue::Geometry(_) => ValueItem::Opaque,
        GeoValue::Encoded(encoded) => ValueItem::Literal(LiteralValue::Text(Cow::Borrowed(encoded.as_str()))),
    }
}

impl<'a, M: QueriedMode> Iterator for FamilyValues<'a, M> {
    type Item = ValueItem<'a, &'a ValueNode<M>>;

    fn next(&mut self) -> Option<ValueItem<'a, &'a ValueNode<M>>> {
        match self {
            FamilyValues::Expanded(values, value_type) => values.next().map(|value| item_of(value, *value_type)),
            FamilyValues::Objects(objects) => objects.next().map(|object| ValueItem::Reference(object.as_str())),
            FamilyValues::Terms(terms) => terms.next().map(|term| ValueItem::Reference(term.as_str())),
            FamilyValues::Entries(entries) => entries
                .find_map(|entry| M::entry_text(entry.text()))
                .map(|text| ValueItem::Literal(LiteralValue::Text(Cow::Borrowed(text)))),
            FamilyValues::Json(literals) => literals.next().map(|literal| ValueItem::Json(literal.piece())),
            FamilyValues::ListObjects(objects) => objects.next().map(|object| ValueItem::Reference(object.uri().as_str())),
            FamilyValues::Single(item) => item.take(),
        }
    }
}
