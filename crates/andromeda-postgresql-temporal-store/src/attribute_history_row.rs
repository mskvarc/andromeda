//! One row of the `attribute_history` hypertable.

use crate::{attribute_history_value::AttributeHistoryValue, error::TemporalStoreError, semantic_time_axes::SemanticTimeAxes};
use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey};
use andromeda_temporal::append_intent::TemporalObservationIdentity;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use uuid::Uuid;

/// A row is keyed on its stream (one entity, one attribute name, one dataset) and on the identity of the atomic write that produced it. That write
/// identity together with `recorded_at` is the table's unique key, which is what
/// makes a replayed write collide with the rows it already produced instead of
/// duplicating them; `recorded_at` is therefore taken from the write's own
/// durable record rather than read from the clock a second time.
///
/// `recorded_at` is the operational axis the hypertable partitions on and is
/// never rendered as an NGSI-LD Temporal Property. The axes a query may select
/// live in [`SemanticTimeAxes`] and in the tombstone's deletion instant.
#[derive(Clone, Debug)]
pub struct AttributeHistoryRow {
    /// Instant the broker durably accepted the write; the hypertable's partition column.
    pub recorded_at: DateTime<Utc>,
    /// The atomic write that produced the row and the row's place in it.
    pub identity: TemporalObservationIdentity,
    /// Stream the observation extends.
    pub stream_key: StreamKey,
    /// Attribute family the row was written as. Recorded per row rather than per stream so that an attribute whose type
    /// later changes does not retroactively change the shape of what was
    /// already recorded.
    pub attribute_kind: AttributeKind,
    /// What the row records.
    pub value: AttributeHistoryValue,
    /// NGSI-LD time axes a temporal query may select on this row.
    pub axes: SemanticTimeAxes,
    /// Identifier the instance is published to clients under.
    pub instance: InstanceIdentity,
    /// Catalog identity of the instance's `unitCode`, absent when it declares none.
    pub unit_code_id: Option<UnitCodeId>,
    /// Stored JSON of the instance's metadata, with `unitCode` already lifted into its column.
    pub metadata: Option<Box<RawValue>>,
}

impl AttributeHistoryRow {
    /// UUID of the atomic write that produced the row.
    #[must_use]
    pub const fn write_id(&self) -> Uuid {
        self.identity.write_id.as_uuid()
    }

    /// Position within the atomic write, in the column's width.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when the position exceeds what
    /// the column holds, which means the write carried more entries than the
    /// broker can address rather than that the row is malformed.
    pub fn write_ordinal(&self) -> Result<i32, TemporalStoreError> {
        self.identity.ordinal.as_i32().map_err(TemporalStoreError::WriteOrdinal)
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_history_row::AttributeHistoryRow, attribute_history_value::AttributeHistoryValue, semantic_time_axes::SemanticTimeAxes};
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey};
    use andromeda_temporal::append_intent::TemporalObservationIdentity;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn row(instance: InstanceIdentity, ordinal: WriteOrdinal) -> AttributeHistoryRow {
        AttributeHistoryRow {
            recorded_at: Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant"),
            identity: TemporalObservationIdentity {
                write_id: WriteId::from_uuid(Uuid::nil()),
                ordinal,
            },
            stream_key: StreamKey::try_from(1).expect("positive"),
            attribute_kind: AttributeKind::Property,
            value: AttributeHistoryValue::Boolean(true),
            axes: SemanticTimeAxes::default(),
            instance,
            unit_code_id: None,
            metadata: None,
        }
    }

    #[test]
    fn the_write_identity_is_carried_through_to_the_stored_columns() {
        let row = row(InstanceIdentity::Generated(Uuid::from_u128(7)), WriteOrdinal::at(3));
        pretty_assertions::assert_eq!(row.write_id(), Uuid::nil());
        pretty_assertions::assert_eq!(row.write_ordinal().expect("a representable ordinal"), 3);
    }

    #[test]
    fn an_ordinal_beyond_the_column_width_is_refused_rather_than_wrapped() {
        let row = row(InstanceIdentity::Generated(Uuid::nil()), WriteOrdinal::at(u32::MAX));
        assert!(row.write_ordinal().is_err());
    }
}
