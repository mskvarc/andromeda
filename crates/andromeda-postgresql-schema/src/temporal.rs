//! The relations only a temporal statement reads.
//!
//! The temporal catalogue, the history hypertable and the attribute-name
//! dictionary. `temporal_entities` is distinct from `entities`, and never
//! joined to it: Core Entity deletion (clause 5.6.6) removes the current-state
//! row while the temporal evolution survives until a Temporal Evolution
//! deletion (clause 5.6.16) removes it, so a temporal query that reached the
//! entity URI or the entity types through `entities` would stop answering for
//! an entity whose history it is supposed to keep serving.

use sea_query::Iden;

/// The `temporal_entities` table - the entity grain temporal state hangs off.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntities {
    /// Table name: `temporal_entities`
    Table,
    /// Surrogate key the stream catalogue and scope history reference
    TemporalEntityKey,
    /// Entity URI, carried independently of `entities`
    Id,
    /// Array of `entity_type_names` keys, one per type the entity carries
    EntityTypeNameKeys,
    /// Instant the Temporal Evolution began (clause 4.8 `createdAt`), never moved
    CreatedAt,
    /// Instant the Temporal Evolution was last extended (clause 4.8 `modifiedAt`)
    ModifiedAt,
}

/// The `temporal_entity_type_members` table - one row per type one Temporal
/// Evolution is recorded under, keyed by type and then identifier.
///
/// A temporal type selection walks one key range of it in identifier order, so
/// a page of a type reads no Evolution of another type.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntityTypeMembers {
    /// Table name: `temporal_entity_type_members`
    Table,
    /// Foreign key to `temporal_entities`, cascading on the Evolution's deletion
    TemporalEntityKey,
    /// The `entity_type_names` key of the type recorded
    EntityTypeNameKey,
    /// The Entity's identifier, copied from `temporal_entities` so the walk is ordered by it
    Id,
}

/// The `temporal_attribute_streams` table - the stable grain a history row belongs to.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAttributeStreams {
    /// Table name: `temporal_attribute_streams`
    Table,
    /// Generated surrogate key, referenced by `attribute_history.stream_key`
    StreamKey,
    /// Foreign key to `temporal_entities`
    TemporalEntityKey,
    /// The Entity's identifier, copied from `temporal_entities` so the
    /// streams of one attribute are walked in identifier order
    Id,
    /// Foreign key to `attribute_names`
    AttributeNameKey,
    /// Optional dataset identifier (clause 4.5.5)
    DatasetId,
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
    /// Boolean value, when `value_kind` selects it
    ValueBoolean,
    /// Geometry value, when `value_kind` selects it
    ValueGeometry,
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
    /// Foreign key to `temporal_entities`
    TemporalEntityKey,
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
    /// Foreign key to `temporal_entities`
    TemporalEntityKey,
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
    /// Bare UUID of a broker-minted `instanceId`
    GeneratedInstanceId,
    /// Verbatim text of a client-supplied `instanceId`
    VerbatimInstanceId,
    /// The scope as an `ltree` label path, absent on a tombstone
    ScopePath,
    /// The scope as it was written, absent on a tombstone
    VerbatimScopePath,
}

#[cfg(test)]
mod tests {
    use crate::temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities, TemporalEntityTypeMembers};
    use sea_query::Iden;

    #[test]
    fn temporal_entity_names() {
        pretty_assertions::assert_eq!(TemporalEntities::Table.to_string(), "temporal_entities");
        pretty_assertions::assert_eq!(TemporalEntities::TemporalEntityKey.to_string(), "temporal_entity_key");
        pretty_assertions::assert_eq!(TemporalEntities::Id.to_string(), "id");
        pretty_assertions::assert_eq!(TemporalEntities::EntityTypeNameKeys.to_string(), "entity_type_name_keys");
    }

    #[test]
    fn temporal_entity_type_member_names() {
        pretty_assertions::assert_eq!(TemporalEntityTypeMembers::Table.to_string(), "temporal_entity_type_members");
        pretty_assertions::assert_eq!(TemporalEntityTypeMembers::TemporalEntityKey.to_string(), "temporal_entity_key");
        pretty_assertions::assert_eq!(TemporalEntityTypeMembers::EntityTypeNameKey.to_string(), "entity_type_name_key");
        pretty_assertions::assert_eq!(TemporalEntityTypeMembers::Id.to_string(), "id");
    }

    #[test]
    fn temporal_attribute_stream_names() {
        pretty_assertions::assert_eq!(TemporalAttributeStreams::Table.to_string(), "temporal_attribute_streams");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::TemporalEntityKey.to_string(), "temporal_entity_key");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::AttributeNameKey.to_string(), "attribute_name_key");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::DatasetId.to_string(), "dataset_id");
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
            ]
            .map(|column| column.to_string()),
            ["value_type_key", "object_type_keys", "proof_seals", "expires_at", "sub_attributes"]
        );
    }
}
