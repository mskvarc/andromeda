//! The relations only a temporal statement reads.
//!
//! The stream catalogue, the history hypertables and the instance claims. The
//! Entity a Temporal Evolution belongs to is its `entities` row, which a core
//! deletion (clause 5.6.6) keeps while the Evolution exists and a Temporal
//! Evolution deletion (clause 5.6.16) gives up.

use sea_query::Iden;

/// The `temporal_attribute_streams` table - the stable grain a history row belongs to.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAttributeStreams {
    /// Table name: `temporal_attribute_streams`
    Table,
    /// Generated surrogate key, referenced by `attribute_history.stream_key`
    StreamKey,
    /// Foreign key to `entities`
    EntityKey,
    /// Foreign key to `attribute_names`
    AttributeNameKey,
    /// Optional dataset identifier (clause 4.5.5)
    DatasetId,
    /// The bounding box of every geometry the stream ever recorded, `NULL`
    /// until it records one; it only grows, so it holds every geometry the
    /// stream holds now
    ValueExtent,
    /// Every combination of attribute family and refined value shape the
    /// stream's instances carry, one bit each, which clause 5.7.4.4 decides
    /// aggregation eligibility from
    ShapeMask,
}

/// The `stream_type_census` table - a signed-delta ledger of how many streams
/// of each attribute the Temporal Evolutions of each type hold, which names
/// the types a temporal selection by attribute walks.
#[derive(Iden, Clone, Copy)]
pub enum StreamTypeCensus {
    /// Table name: `stream_type_census`
    Table,
    /// The `entity_type_names` key of a type the streams' Evolutions are
    /// recorded under
    EntityTypeNameKey,
    /// The `attribute_names` key of the streams' attribute
    AttributeNameKey,
    /// The signed change in the number of streams
    StreamDelta,
}

/// The `attribute_history` hypertable - stores historical attribute instances.
#[derive(Iden, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeHistory {
    /// Table name: `attribute_history`
    Table,
    /// Key of the `temporal_attribute_streams` row (no foreign key on a hypertable)
    StreamKey,
    /// Write identity of the mutation that produced the row (clause 5.6.7)
    WriteId,
    /// Ordinal of the row within its write, part of the replay-collision key
    WriteOrdinal,
    /// Operational instant the row was written, never rewritten
    RecordedAt,
    /// Instant the row was first recorded at (clause 4.8 `createdAt`)
    CreatedAt,
    /// NGSI-LD `modifiedAt` axis (clause 4.8), advanced by a modification
    ModifiedAt,
    /// Observation instant the instance declares (clause 4.8 `observedAt`)
    ObservedAt,
    /// Instant a tombstone row records the instance ended, when it does
    DeletedAt,
    /// Bare UUID of a broker-minted `instanceId`
    GeneratedInstanceId,
    /// Verbatim text of a client-supplied `instanceId`
    VerbatimInstanceId,
    /// Attribute family code of the stored instance
    AttributeKind,
    /// Value shape code, pinning which typed value column is populated
    ValueKind,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value, when `value_kind` selects it
    ValueString,
    /// The `@language` of a language-tagged string
    ValueLanguage,
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// Geometry value as written, when `value_kind` selects it
    ValueGeometry,
    /// The `ST_MakeValid` form of an invalid written geometry, NULL when the
    /// written one is valid; spatial predicates judge this form when present
    ValueGeometryValid,
    /// The `bbox` and foreign members of the geometry value, when it has any
    ValueGeometryMembers,
    /// JSON object or array value, when `value_kind` selects it
    ValueObject,
    /// `DateTime` value, when `value_kind` selects it
    ValueDateTime,
    /// URI value, when `value_kind` selects it
    ValueUri,
    /// `cefact-units` identity of the instance's `unitCode` (clause 4.5.2), carrying no foreign key
    UnitCodeId,
    /// `entity_type_names` key of the instance's `valueType` (clause 4.5.2)
    ValueTypeKey,
    /// `entity_type_names` key of the `@type` a typed scalar value declares
    ValueDatatypeKey,
    /// `entity_type_names` keys of the instance's `objectType` list (clause 4.5.3), in written order
    ObjectTypeKeys,
    /// The instance's `entityIdSealed` and `entityTypeSealed` members (clause 4.5.2.2)
    ProofSeals,
    /// The instance's `expiresAt` (clause 4.22) as it was recorded
    ExpiresAt,
    /// The instance's sub-attributes as `sub_attribute` elements in pre-order
    SubAttributes,
}

/// The `attribute_history_instance_claims` table - the uniqueness reservations a
/// client-supplied `instanceId` holds so a verbatim identifier stays singular.
#[derive(Iden, Clone, Copy)]
pub enum AttributeHistoryInstanceClaims {
    /// Table name: `attribute_history_instance_claims`
    Table,
    /// The verbatim `instanceId` the claim reserves
    VerbatimInstanceId,
    /// Foreign key to `entities`
    EntityKey,
    /// Foreign key to `attribute_names`
    AttributeNameKey,
}

/// The `scope_history` table - the temporal evolution of an entity's scopes.
#[derive(Iden, Clone, Copy)]
pub enum ScopeHistory {
    /// Table name: `scope_history`
    Table,
    /// Operational instant the row was written, the hypertable partition column
    RecordedAt,
    /// The `entities` key of the Entity whose Scope the row records
    EntityKey,
    /// Write identity of the mutation that produced the row (clause 5.6.7)
    WriteId,
    /// Ordinal of the row within its write, part of the replay-collision key
    WriteOrdinal,
    /// Value shape code of the scope observation
    ValueKind,
    /// Observation instant the row declares (clause 4.8 `observedAt`)
    ObservedAt,
    /// Instant the row was first recorded at (clause 4.8 `createdAt`)
    CreatedAt,
    /// Instant the row was last altered at (clause 4.8 `modifiedAt`)
    ModifiedAt,
    /// Deletion instant, present only on a tombstone row
    DeletedAt,
    /// The scope as an `ltree` label path, absent on a tombstone
    ScopePath,
    /// The scope as it was written, absent on a tombstone
    VerbatimScopePath,
}

#[cfg(test)]
mod tests {
    use crate::temporal::{AttributeHistory, ScopeHistory, StreamTypeCensus, TemporalAttributeStreams};
    use sea_query::Iden;

    #[test]
    fn temporal_attribute_stream_names() {
        pretty_assertions::assert_eq!(TemporalAttributeStreams::Table.to_string(), "temporal_attribute_streams");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::EntityKey.to_string(), "entity_key");
        pretty_assertions::assert_eq!(ScopeHistory::EntityKey.to_string(), "entity_key");
        pretty_assertions::assert_eq!(
            [
                StreamTypeCensus::Table,
                StreamTypeCensus::EntityTypeNameKey,
                StreamTypeCensus::AttributeNameKey,
                StreamTypeCensus::StreamDelta,
            ]
            .map(|column| column.to_string()),
            ["stream_type_census", "entity_type_name_key", "attribute_name_key", "stream_delta"]
        );
        pretty_assertions::assert_eq!(TemporalAttributeStreams::AttributeNameKey.to_string(), "attribute_name_key");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::DatasetId.to_string(), "dataset_id");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::ShapeMask.to_string(), "shape_mask");
    }

    #[test]
    fn attribute_history_names() {
        pretty_assertions::assert_eq!(AttributeHistory::Table.to_string(), "attribute_history");
        pretty_assertions::assert_eq!(AttributeHistory::StreamKey.to_string(), "stream_key");
        pretty_assertions::assert_eq!(
            [
                AttributeHistory::ValueTypeKey,
                AttributeHistory::ObjectTypeKeys,
                AttributeHistory::ProofSeals,
                AttributeHistory::ExpiresAt,
                AttributeHistory::SubAttributes,
                AttributeHistory::ValueGeometryValid,
                AttributeHistory::ValueDatatypeKey,
                AttributeHistory::ValueLanguage,
            ]
            .map(|column| column.to_string()),
            [
                "value_type_key",
                "object_type_keys",
                "proof_seals",
                "expires_at",
                "sub_attributes",
                "value_geometry_valid",
                "value_datatype_key",
                "value_language"
            ]
        );
    }
}
