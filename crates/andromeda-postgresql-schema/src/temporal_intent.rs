//! The composite types a temporal append intent travels and waits in.

use sea_query::Iden;

/// The fields of the `temporal_entry` composite type - one entry of a
/// temporal append intent, in the type's declaration order.
///
/// Behind the three fields that name the entry's grain, the fields are the
/// `attribute_history` columns but the write identity and the stream, in the
/// table's column order, then the two Scope fields.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntry {
    /// Type name: `temporal_entry`
    #[iden = "temporal_entry"]
    Type,
    /// 1 an attribute observation, 2 the end of an attribute instance, 3 a Scope entry
    EntryKind,
    /// `attribute_names` key of the attribute, NULL for a Scope entry
    AttributeNameKey,
    /// Dataset identifier of the attribute (clause 4.5.5); NULL is the default instance
    DatasetId,
    /// Attribute family code
    AttributeKind,
    /// Value shape code; 0 is the end of an instance
    ValueKind,
    /// NGSI-LD `observedAt` axis (clause 5.2.21)
    ObservedAt,
    /// NGSI-LD `createdAt` axis (clause 4.8)
    CreatedAt,
    /// NGSI-LD `modifiedAt` axis (clause 4.8)
    ModifiedAt,
    /// NGSI-LD `deletedAt` axis, present only on the end of an instance
    DeletedAt,
    /// `DateTime` value, when `value_kind` selects it
    ValueDateTime,
    /// The instance's clause 4.22 `expiresAt`
    ExpiresAt,
    /// Broker-minted instance identifier
    GeneratedInstanceId,
    /// `entity_type_names` key of the clause 4.5.2 `valueType`
    ValueTypeKey,
    /// `entity_type_names` key of the `@type` a typed scalar value declares
    ValueDatatypeKey,
    /// `cefact-units` identity of the instance's `unitCode` (clause 4.5.2)
    UnitCodeId,
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// Instance identifier exactly as the client wrote it (clause 4.5.7)
    VerbatimInstanceId,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value, when `value_kind` selects it
    ValueString,
    /// The `@language` of a language-tagged string
    ValueLanguage,
    /// Geometry value, when `value_kind` selects it
    ValueGeometry,
    /// The `GeoJSON` members a geometry carries beside its coordinates
    ValueGeometryMembers,
    /// Structured value
    ValueObject,
    /// URI value
    ValueUri,
    /// `entity_type_names` keys of the clause 4.5.3 `objectType`, in written order
    ObjectTypeKeys,
    /// The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members
    ProofSeals,
    /// The instance's sub-attributes as `sub_attribute` elements
    SubAttributes,
    /// `ltree` label path of a Scope entry, NULL for a Scope deletion
    ScopePath,
    /// The Scope as written, NULL for a Scope deletion
    VerbatimScopePath,
}

/// The fields of the `temporal_intent` composite type - one intent of a
/// commit as the enqueue binds it, in the type's declaration order.
#[derive(Iden, Clone, Copy)]
pub enum TemporalIntent {
    /// Type name: `temporal_intent`
    #[iden = "temporal_intent"]
    Type,
    /// Entity the intent appends temporal history for
    EntityId,
    /// `entity_type_names` keys the Temporal Evolution is recorded under
    EntityTypeNameKeys,
    /// Write ordinal of the intent's first entry
    BaseOrdinal,
    /// The intent's entries in write order
    Entries,
    /// `TemporalIntentReach` codes of the application statements' needs the entries meet
    EntryNeeds,
}

#[cfg(test)]
mod tests {
    use crate::temporal_intent::{TemporalEntry, TemporalIntent};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn temporal_intent_field_names_in_order() {
        let fields = [
            TemporalIntent::EntityId,
            TemporalIntent::EntityTypeNameKeys,
            TemporalIntent::BaseOrdinal,
            TemporalIntent::Entries,
            TemporalIntent::EntryNeeds,
        ]
        .map(|field| field.to_string());
        assert_eq!(TemporalIntent::Type.to_string(), "temporal_intent");
        assert_eq!(fields, ["entity_id", "entity_type_name_keys", "base_ordinal", "entries", "entry_needs",]);
    }

    #[test]
    fn temporal_entry_names_its_type() {
        assert_eq!(TemporalEntry::Type.to_string(), "temporal_entry");
        assert_eq!(TemporalEntry::VerbatimScopePath.to_string(), "verbatim_scope_path");
    }
}
