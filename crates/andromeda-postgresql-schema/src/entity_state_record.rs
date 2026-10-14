//! The composite types one Entity's state travels in outside its
//! `entities` and `current_attributes` rows.

use sea_query::Iden;

/// The fields of the `attribute_instance_record` composite type - one
/// top-level attribute instance of a state, in the type's declaration order.
///
/// The fields are the `current_attributes` columns but `entity_key`, in the
/// table's column order and with the same types, so a record and a current
/// row hold one instance alike.
#[derive(Iden, Clone, Copy)]
pub enum AttributeInstanceRecord {
    /// Type name: `attribute_instance_record`
    #[iden = "attribute_instance_record"]
    Type,
    /// Instant the instance was created (clause 4.8 `createdAt`)
    CreatedAt,
    /// Instant the instance was last modified (clause 4.8 `modifiedAt`)
    ModifiedAt,
    /// `attribute_names` key of the attribute
    AttributeNameKey,
    /// Written position of the instance among the state's instances, from 0
    Ordinal,
    /// Attribute family code
    AttributeKind,
    /// Value shape code, pinning which typed value field is populated
    ValueKind,
    /// `DateTime` value, when `value_kind` selects it
    ValueDateTime,
    /// The instance's clause 4.5.2 `observedAt`, NULL when it carries none
    ObservedAt,
    /// The instance's clause 4.22 `expiresAt`, NULL when it never expires
    ExpiresAt,
    /// `entity_type_names` key of the datatype IRI a typed literal declares
    ValueDatatypeKey,
    /// `entity_type_names` key of the clause 4.5.2 `valueType`
    ValueTypeKey,
    /// `cefact-units` identity of the instance's `unitCode` (clause 4.5.2)
    UnitCodeId,
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// Optional dataset identifier (clause 4.5.5); NULL is the default instance
    DatasetId,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value within the `current_attributes` B-tree bound
    ValueString,
    /// Language tag of a language-tagged String
    ValueLanguage,
    /// Geometry value, when `value_kind` selects it
    ValueGeometry,
    /// The `GeoJSON` members a geometry carries beside its coordinates
    ValueGeometryMembers,
    /// Every value that is not one scalar, in the structured value encoding
    ValueObject,
    /// URI value within the `current_attributes` B-tree bound
    ValueUri,
    /// String value past the `current_attributes` B-tree bound
    ValueLongText,
    /// URI value past the `current_attributes` B-tree bound
    ValueLongUri,
    /// `entity_type_names` keys of the clause 4.5.3 `objectType`, in written order
    ObjectTypeKeys,
    /// The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members
    ProofSeals,
    /// The instance's sub-attributes as `sub_attribute` elements, NULL when it
    /// has none
    SubAttributes,
}

/// The fields of the `entity_state_record` composite type - one Entity's
/// state beside its identifier, in the type's declaration order.
#[derive(Iden, Clone, Copy)]
pub enum EntityStateRecord {
    /// Type name: `entity_state_record`
    #[iden = "entity_state_record"]
    Type,
    /// `entity_type_names` keys of the Entity's types, in written order
    EntityTypeNameKeys,
    /// The Entity's Scopes as written (clause 4.18), empty when it has none
    ScopePaths,
    /// The Entity's clause 4.22 `expiresAt`, NULL when it never expires
    ExpiresAt,
    /// The Entity's top-level attribute instances as
    /// `attribute_instance_record`s, in ordinal order
    Instances,
}

#[cfg(test)]
mod tests {
    use crate::{
        current::CurrentAttributes,
        entity_state_record::{AttributeInstanceRecord, EntityStateRecord},
    };
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn attribute_instance_record_field_names_follow_the_current_attributes_columns() {
        let fields = [
            AttributeInstanceRecord::CreatedAt,
            AttributeInstanceRecord::ModifiedAt,
            AttributeInstanceRecord::AttributeNameKey,
            AttributeInstanceRecord::Ordinal,
            AttributeInstanceRecord::AttributeKind,
            AttributeInstanceRecord::ValueKind,
            AttributeInstanceRecord::ValueDateTime,
            AttributeInstanceRecord::ObservedAt,
            AttributeInstanceRecord::ExpiresAt,
            AttributeInstanceRecord::ValueDatatypeKey,
            AttributeInstanceRecord::ValueTypeKey,
            AttributeInstanceRecord::UnitCodeId,
            AttributeInstanceRecord::ValueBoolean,
            AttributeInstanceRecord::DatasetId,
            AttributeInstanceRecord::ValueNumber,
            AttributeInstanceRecord::ValueString,
            AttributeInstanceRecord::ValueLanguage,
            AttributeInstanceRecord::ValueGeometry,
            AttributeInstanceRecord::ValueGeometryMembers,
            AttributeInstanceRecord::ValueObject,
            AttributeInstanceRecord::ValueUri,
            AttributeInstanceRecord::ValueLongText,
            AttributeInstanceRecord::ValueLongUri,
            AttributeInstanceRecord::ObjectTypeKeys,
            AttributeInstanceRecord::ProofSeals,
            AttributeInstanceRecord::SubAttributes,
        ]
        .map(|field| field.to_string());
        let columns = [
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            CurrentAttributes::ValueDateTime,
            CurrentAttributes::ObservedAt,
            CurrentAttributes::ExpiresAt,
            CurrentAttributes::ValueDatatypeKey,
            CurrentAttributes::ValueTypeKey,
            CurrentAttributes::UnitCodeId,
            CurrentAttributes::ValueBoolean,
            CurrentAttributes::DatasetId,
            CurrentAttributes::ValueNumber,
            CurrentAttributes::ValueString,
            CurrentAttributes::ValueLanguage,
            CurrentAttributes::ValueGeometry,
            CurrentAttributes::ValueGeometryMembers,
            CurrentAttributes::ValueObject,
            CurrentAttributes::ValueUri,
            CurrentAttributes::ValueLongText,
            CurrentAttributes::ValueLongUri,
            CurrentAttributes::ObjectTypeKeys,
            CurrentAttributes::ProofSeals,
            CurrentAttributes::SubAttributes,
        ]
        .map(|column| column.to_string());
        assert_eq!(AttributeInstanceRecord::Type.to_string(), "attribute_instance_record");
        assert_eq!(fields, columns);
    }

    #[test]
    fn entity_state_record_field_names_in_order() {
        let fields = [
            EntityStateRecord::EntityTypeNameKeys,
            EntityStateRecord::ScopePaths,
            EntityStateRecord::ExpiresAt,
            EntityStateRecord::Instances,
        ]
        .map(|field| field.to_string());
        assert_eq!(EntityStateRecord::Type.to_string(), "entity_state_record");
        assert_eq!(fields, ["entity_type_name_keys", "scope_paths", "expires_at", "instances"]);
    }
}
