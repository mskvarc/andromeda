//! The family of an attribute instance and the value that family carries
//! (clause 4.5).
//!
//! Exactly one family per instance. Each variant holds the members only that
//! family defines, so a `unitCode` on a Relationship or an `objectType` on a
//! Property is not a state this type can reach (clauses 4.5.2.2, 4.5.3.2,
//! 4.5.18.2, 4.5.20.2, 4.5.21.2, 4.5.22.2, 4.5.24.2).

use crate::{
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    json_literal::JsonLiteral,
    language_entries::LanguageEntries,
    list_object::ListObject,
    non_empty::NonEmpty,
    unit_code::UnitCode,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, reference::NgsiLdReference, uri::Uri};

/// One instance's family and value.
#[derive(Clone, Debug, PartialEq)]
pub enum FamilyValue {
    /// A Property (clause 4.5.2): `hasValue` may hold several values.
    Property {
        /// The values of `hasValue`, in order.
        values: NonEmpty<ExpandedValue>,
        /// The `unitCode`.
        unit_code: Option<UnitCode>,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A `GeoProperty` (clause 4.7).
    GeoProperty {
        /// The geometry of `hasValue`.
        geometry: GeoValue,
        /// The `unitCode` clause 4.7.3 carries over from the Property.
        unit_code: Option<UnitCode>,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A Relationship (clause 4.5.3), its targets in order.
    Relationship {
        /// The targets of `hasObject`.
        objects: NonEmpty<Uri>,
        /// The `objectType` entries.
        object_types: Vec<NgsiLdReference>,
    },
    /// A `LanguageProperty` (clause 4.5.18).
    LanguageProperty {
        /// The entries of `hasLanguageMap`.
        entries: LanguageEntries,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A `VocabProperty` (clause 4.5.20).
    VocabProperty {
        /// The terms of `hasVocab`.
        vocab: NonEmpty<NgsiLdReference>,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A `JsonProperty` (clause 4.5.24).
    JsonProperty {
        /// The literals of `hasJSON`.
        json: NonEmpty<JsonLiteral>,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A `ListProperty` (clause 4.5.21).
    ListProperty {
        /// The items of `hasValueList`, in order.
        items: Vec<ExpandedValue>,
        /// The `unitCode` clause 4.5.21.2 inherits from the Property.
        unit_code: Option<UnitCode>,
        /// The `valueType`.
        value_type: Option<NgsiLdReference>,
    },
    /// A `ListRelationship` (clause 4.5.22).
    ListRelationship {
        /// The targets of `hasObjectList`, in order, each in its stored
        /// spelling.
        objects: Vec<ListObject>,
        /// The `objectType` entries.
        object_types: Vec<NgsiLdReference>,
    },
}

impl FamilyValue {
    /// The family.
    #[must_use]
    pub const fn family(&self) -> AttributeType {
        match self {
            FamilyValue::Property { .. } => AttributeType::Property,
            FamilyValue::GeoProperty { .. } => AttributeType::GeoProperty,
            FamilyValue::Relationship { .. } => AttributeType::Relationship,
            FamilyValue::LanguageProperty { .. } => AttributeType::LanguageProperty,
            FamilyValue::VocabProperty { .. } => AttributeType::VocabProperty,
            FamilyValue::JsonProperty { .. } => AttributeType::JsonProperty,
            FamilyValue::ListProperty { .. } => AttributeType::ListProperty,
            FamilyValue::ListRelationship { .. } => AttributeType::ListRelationship,
        }
    }

    /// The `unitCode`, for the families that define one.
    #[must_use]
    pub const fn unit_code(&self) -> Option<UnitCode> {
        match self {
            FamilyValue::Property { unit_code, .. } | FamilyValue::GeoProperty { unit_code, .. } | FamilyValue::ListProperty { unit_code, .. } => *unit_code,
            FamilyValue::Relationship { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListRelationship { .. } => None,
        }
    }

    /// The `valueType`, for the families that define one.
    #[must_use]
    pub const fn value_type(&self) -> Option<&NgsiLdReference> {
        match self {
            FamilyValue::Property { value_type, .. }
            | FamilyValue::GeoProperty { value_type, .. }
            | FamilyValue::LanguageProperty { value_type, .. }
            | FamilyValue::VocabProperty { value_type, .. }
            | FamilyValue::JsonProperty { value_type, .. }
            | FamilyValue::ListProperty { value_type, .. } => value_type.as_ref(),
            FamilyValue::Relationship { .. } | FamilyValue::ListRelationship { .. } => None,
        }
    }

    /// The `objectType` entries, for the two relationship families.
    #[must_use]
    pub fn object_types(&self) -> &[NgsiLdReference] {
        match self {
            FamilyValue::Relationship { object_types, .. } | FamilyValue::ListRelationship { object_types, .. } => object_types,
            FamilyValue::Property { .. }
            | FamilyValue::GeoProperty { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListProperty { .. } => &[],
        }
    }

    /// The geometry, for a `GeoProperty`.
    #[must_use]
    pub const fn geometry(&self) -> Option<&GeoValue> {
        match self {
            FamilyValue::GeoProperty { geometry, .. } => Some(geometry),
            FamilyValue::Property { .. }
            | FamilyValue::Relationship { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListProperty { .. }
            | FamilyValue::ListRelationship { .. } => None,
        }
    }
}
