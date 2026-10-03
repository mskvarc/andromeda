//! The value a written attribute instance carries, by the member carrying it.
//!
//! A written fragment need not declare its family (clause 5.6.4), so its value
//! is told apart by the member it is written under rather than by the family.
//! A `GeoProperty`'s `value` is read as a geometry straight away when the
//! instance declares the family; otherwise it stays a Property value until a
//! write applies it to a stored `GeoProperty`.

use crate::{
    document::value_carrier::ValueCarrier,
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    json_literal::JsonLiteral,
    language_entries::LanguageEntries,
    list_object::ListObject,
    non_empty::NonEmpty,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, uri::Uri};

/// A written value, by its carrying member.
#[derive(Clone, Debug, PartialEq)]
pub enum WrittenValue {
    /// `value` of a Property, or of a fragment that declares no family.
    ///
    /// No value is the empty array, which clause 3.1 makes an NGSI-LD Value
    /// like any other array; a fragment carrying no `value` member at all is
    /// [`ValueSlot::Absent`] instead.
    Value(Vec<ExpandedValue<Fragment>>),
    /// `value` of an instance declaring itself a `GeoProperty`.
    Geometry(GeoValue),
    /// `object`.
    Object(NonEmpty<Uri>),
    /// `languageMap`; a merge may delete one language with the NGSI-LD Null.
    LanguageMap(LanguageEntries<Fragment>),
    /// `vocab`.
    Vocab(NonEmpty<NgsiLdReference>),
    /// `json`.
    Json(NonEmpty<JsonLiteral>),
    /// `valueList`.
    ValueList(Vec<ExpandedValue<Fragment>>),
    /// `objectList`.
    ObjectList(Vec<ListObject>),
}

impl WrittenValue {
    /// The member the value is written under.
    #[must_use]
    pub const fn carrier(&self) -> ValueCarrier {
        match self {
            WrittenValue::Value(_) | WrittenValue::Geometry(_) => ValueCarrier::Value,
            WrittenValue::Object(_) => ValueCarrier::Object,
            WrittenValue::LanguageMap(_) => ValueCarrier::LanguageMap,
            WrittenValue::Vocab(_) => ValueCarrier::Vocab,
            WrittenValue::Json(_) => ValueCarrier::Json,
            WrittenValue::ValueList(_) => ValueCarrier::ValueList,
            WrittenValue::ObjectList(_) => ValueCarrier::ObjectList,
        }
    }
}

/// What a written instance says about its value.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueSlot {
    /// The instance carries no value member (a partial update that touches
    /// only other members, clause 5.5.8).
    Absent,
    /// The instance sets its value.
    Set(WrittenValue),
    /// The value member is the family's deletion encoding (clauses 4.5.2.2,
    /// 4.5.3.2, 4.5.18, 4.5.20.2, 4.5.21.2, 4.5.22.2, 4.5.24.2): delete the
    /// instance.
    Remove(ValueCarrier),
}
