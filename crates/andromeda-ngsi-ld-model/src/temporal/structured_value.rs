//! The structured value of one temporal observation.
//!
//! A history row keeps a value in a typed column whenever one fits: a number,
//! a string, a boolean, a geometry, a `DateTime`, a single URI. Everything else
//! an attribute family can hold (clause 4.5) is a structured value: the several
//! values of an array-valued Property, the value node of an object-valued one,
//! the targets of a Relationship naming several (clause 4.5.3.2), the terms of
//! a `VocabProperty` naming several (clause 4.5.20), a language map (clause
//! 4.5.18), a JSON literal (clause 4.5.24), the items of a `ListProperty`
//! (clause 4.5.21) and the targets of a `ListRelationship` (clause 4.5.22).
//!
//! Values are held in the written mode of the model: a Temporal Evolution
//! records what a client wrote, and clause 5.5.4 exempts "the temporal
//! evolution" from the prohibition of the NGSI-LD Null, so a recorded value may
//! hold one where the client put one.

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_entity::{
    document::written_value::WrittenValue,
    expanded_value::ExpandedValue,
    language_entries::LanguageEntries,
    list_object::ListObject,
    non_empty::NonEmpty,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, uri::Uri};

/// The structured value of one observation, by what its family holds.
#[derive(Clone, Debug, PartialEq)]
pub enum StructuredValue {
    /// The value of a Property (or of a `GeoProperty` recorded before its
    /// value had to be a geometry) that no typed column holds: the value node
    /// of an object-valued one, or the values of an array-valued one, whose
    /// target value clause 4.9 makes the whole array. No value is the empty
    /// array (clause 3.1).
    Values(Vec<ExpandedValue<Fragment>>),
    /// The targets of a Relationship naming several (clause 4.5.3.2: "a URI or
    /// array of URIs"), in order.
    Objects(NonEmpty<Uri>),
    /// The terms of a `VocabProperty` naming several (clause 4.5.20), in order.
    Vocabs(NonEmpty<NgsiLdReference>),
    /// The language map of a `LanguageProperty` (clause 4.5.18).
    LanguageMap(LanguageEntries<Fragment>),
    /// The JSON literal of a `JsonProperty` (clause 4.5.24).
    Json(JsonLiteral),
    /// The items of a `ListProperty` (clause 4.5.21), in order.
    List(Vec<ExpandedValue<Fragment>>),
    /// The targets of a `ListRelationship` (clause 4.5.22), in order.
    ObjectList(Vec<Uri>),
}

impl StructuredValue {
    /// The value as the written member of an instance carries it.
    ///
    /// A `ListRelationship` target is written as the normalized member clause
    /// 4.5.22.2 mandates, `{"object": uri}`: a temporal retrieval answers in
    /// the normalized form (Table 5.2.37-1 reserves the bare URI to the concise
    /// one).
    #[must_use]
    pub fn into_written(self) -> WrittenValue {
        match self {
            StructuredValue::Values(values) => WrittenValue::Value(values),
            StructuredValue::Objects(targets) => WrittenValue::Object(targets),
            StructuredValue::Vocabs(terms) => WrittenValue::Vocab(terms),
            StructuredValue::LanguageMap(entries) => WrittenValue::LanguageMap(entries),
            StructuredValue::Json(literal) => WrittenValue::Json(NonEmpty::one(literal)),
            StructuredValue::List(items) => WrittenValue::ValueList(items),
            StructuredValue::ObjectList(targets) => WrittenValue::ObjectList(targets.into_iter().map(ListObject::Wrapped).collect()),
        }
    }
}
