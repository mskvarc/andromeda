//! One row of the rebuilt `attribute_history` hypertable.

use crate::{attribute_history_value::AttributeHistoryValue, error::TemporalStoreError, semantic_time_axes::SemanticTimeAxes};
use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_id::StreamId};
use andromeda_temporal::append_intent::TemporalObservationIdentity;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

/// One attribute-history observation as the hypertable stores it.
///
/// The row is keyed on the stream — one entity, one attribute name, one dataset
/// — and on the identity of the atomic write that produced it. That write
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
    /// The instant the broker durably accepted the write this row belongs to.
    pub recorded_at: DateTime<Utc>,
    /// The atomic write that produced this row, and this row's place in it.
    pub identity: TemporalObservationIdentity,
    /// The stream this observation extends.
    pub stream_id: StreamId,
    /// The attribute family the row was written as.
    ///
    /// Recorded per row rather than per stream so that an attribute whose type
    /// later changes does not retroactively change the shape of what was
    /// already recorded.
    pub attribute_kind: AttributeKind,
    /// What the row records.
    pub value: AttributeHistoryValue,
    /// The NGSI-LD time axes a temporal query may select on this row.
    pub axes: SemanticTimeAxes,
    /// The identifier this instance is published to clients under.
    pub instance: InstanceIdentity,
    /// Catalog identity of the instance's `unitCode`, absent when it declares none.
    pub unit_code_id: Option<UnitCodeId>,
    /// Retained expanded members, with `unitCode` already lifted into its column.
    pub metadata: Option<Value>,
}

impl AttributeHistoryRow {
    /// The UUID of the atomic write that produced this row.
    pub const fn write_id(&self) -> Uuid {
        self.identity.write_id.as_uuid()
    }

    /// This row's position within its atomic write, in the column's width.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when the position exceeds what
    /// the column holds, which means the write carried more entries than the
    /// broker can address rather than that the row is malformed.
    pub fn write_ordinal(&self) -> Result<i32, TemporalStoreError> {
        self.identity
            .ordinal
            .as_i32()
            .map_err(|error| TemporalStoreError::Validation(format!("temporal write ordinal {} is out of range: {error}", self.identity.ordinal)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_history_row::AttributeHistoryRow, attribute_history_value::AttributeHistoryValue, semantic_time_axes::SemanticTimeAxes};
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_id::StreamId};
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
            stream_id: StreamId::from(Uuid::nil()),
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
