//! What a matcher reads of one attribute instance through the typed model: the value its kind holds, its `unitCode`
//! and `valueType`, whether an optional member is present, and a member inside a value by its path.

use crate::{attribute_matcher::Member, geometry::geometry_value};
use cefact_units::UnitCode;
use ngsi_ld::model::{Attribute, AttributeMeta, Output, Value, VocabTerm};

/// The value a Property holds, the items of a `ListProperty` as an array, or the geometry of a `GeoProperty`.
pub(crate) fn value_of(attribute: &Attribute<Output>) -> Option<Value> {
    match attribute {
        Attribute::Property(property) => Some(property.value().clone()),
        Attribute::ListProperty(property) => Some(Value::Array(property.value_list().to_vec())),
        Attribute::GeoProperty(property) => property.value().as_geometry().map(geometry_value),
        Attribute::LanguageProperty(_)
        | Attribute::VocabProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::Relationship(_)
        | Attribute::ListRelationship(_) => None,
    }
}

/// The `unitCode` of a kind that has one.
pub(crate) fn unit_code_of(attribute: &Attribute<Output>) -> Option<&UnitCode> {
    match attribute {
        Attribute::Property(property) => property.unit_code(),
        Attribute::GeoProperty(property) => property.unit_code(),
        Attribute::ListProperty(property) => property.unit_code(),
        Attribute::LanguageProperty(_)
        | Attribute::VocabProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::Relationship(_)
        | Attribute::ListRelationship(_) => None,
    }
}

/// The `valueType` of a kind that has one.
pub(crate) fn value_type_of(attribute: &Attribute<Output>) -> Option<&VocabTerm> {
    match attribute {
        Attribute::Property(property) => property.value_type(),
        Attribute::GeoProperty(property) => property.value_type(),
        Attribute::ListProperty(property) => property.value_type(),
        Attribute::LanguageProperty(property) => property.value_type(),
        Attribute::VocabProperty(property) => property.value_type(),
        Attribute::JsonProperty(property) => property.value_type(),
        Attribute::Relationship(_) | Attribute::ListRelationship(_) => None,
    }
}

/// Whether `member` is present on `meta` or on `attribute`.
pub(crate) fn has_member(attribute: &Attribute<Output>, meta: &AttributeMeta<Output>, member: Member) -> bool {
    match member {
        Member::ObservedAt => meta.observed_at.is_some(),
        Member::ExpiresAt => meta.expires_at.is_some(),
        Member::UnitCode => unit_code_of(attribute).is_some(),
        Member::DatasetId => meta.dataset_id.is_some(),
        Member::ValueType => value_type_of(attribute).is_some(),
        Member::CreatedAt => meta.output.created_at.is_some(),
        Member::ModifiedAt => meta.output.modified_at.is_some(),
        Member::DeletedAt => meta.output.deleted_at.is_some(),
        Member::InstanceId => meta.output.instance_id.is_some(),
        Member::LanguageMap => matches!(attribute, Attribute::LanguageProperty(_)),
        Member::Lang => matches!(attribute, Attribute::Property(property) if property.output().lang.is_some()),
    }
}

/// The member at `path` inside `value`.
pub(crate) fn member_at<'v>(value: &'v Value, path: &[String]) -> Option<&'v Value> {
    path.iter().try_fold(value, |current, segment| match current {
        Value::Object(members) => members.get(segment),
        Value::Array(items) => segment.parse::<usize>().ok().and_then(|index| items.get(index)),
        Value::Boolean(_) | Value::Number(_) | Value::Text(_) | Value::Typed(_) => None,
    })
}
