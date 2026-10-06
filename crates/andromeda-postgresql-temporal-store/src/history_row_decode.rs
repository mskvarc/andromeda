//! Decoding `sqlx::postgres::PgRow` into the backend-neutral
//! [`TemporalHistoryRow`] type declared by the temporal store port.
//!
//! Every column probe, code decode, EWKB-to-GeoJSON conversion and instance
//! identity reassembly lives here so that neither the service layer nor the
//! renderers ever import `sqlx`. The readers for the catalogue columns an
//! `attribute_history` row carries — its attribute name, its `datasetId`, its
//! unit identity — are shared with [`crate::aggregation_row_decode`], whose rows
//! project those same columns.
//!
//! Nothing in this module guesses. The rebuilt history table records which
//! typed column holds a row's value, so the value is read from that column and
//! from no other; a stored code the registry has never assigned fails the
//! decode rather than falling back to a default, because a row written by a
//! later release must be reported as unreadable instead of being silently
//! reinterpreted.

use crate::{
    error::TemporalStoreError,
    history_row_identity::{decode_instance_identity, decode_write_identity, nullable_instant, required_column},
    stored_history_error::{HistoryColumn, HistoryTable, StoredHistoryError, UnitCodeColumn},
    unit_code_metadata::restore_unit_code,
};
use andromeda_ngsi_ld_entity::geometry_value::{ExpandedGeoJsonMembers, GeometryValue};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue, observation_metadata::ObservationMetadata, structured_value::StructuredValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::attribute_type_of, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_temporal::port::history_reader::TemporalHistoryObservation;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use serde_json::value::RawValue;
use sqlx::{Row, postgres::PgRow};

/// The table every row this module decodes comes from.
const TABLE: HistoryTable = HistoryTable::AttributeHistory;

/// Decode one `attribute_history` row into a [`TemporalHistoryRow`].
pub(crate) fn decode_history_row(row: &PgRow) -> Result<TemporalHistoryObservation, TemporalStoreError> {
    let attribute_type = attribute_type_of(decode_attribute_kind(row)?);
    let attribute_name = decode_attribute_name(row)?;
    let value = decode_value(row, attribute_type, &attribute_name)?;
    let metadata = decode_metadata(row, attribute_type, &attribute_name)?;
    let metadata = restore_unit_code(metadata, decode_unit_code_id(row, UnitCodeColumn::UnitCodeId)?)?;

    let temporal_row = TemporalHistoryRow {
        attribute_name,
        attribute_type,
        dataset_id: decode_dataset_id(row)?,
        value,
        // Each axis is read from its own column and from no other. A row that
        // recorded no instant on an axis keeps none, because filling one in
        // from the write clock would invent a match on a Temporal Property the
        // observation does not have (clauses 4.11 and 5.2.21).
        observed_at: nullable_instant(row, HistoryColumn::ObservedAt)?,
        recorded_at: required_column(row, TABLE, HistoryColumn::RecordedAt)?,
        created_at: nullable_instant(row, HistoryColumn::CreatedAt)?,
        modified_at: nullable_instant(row, HistoryColumn::ModifiedAt)?,
        instance: decode_instance_identity(row, TABLE)?,
        metadata,
        partition_total: row.try_get("partition_total").ok(),
    };
    Ok(TemporalHistoryObservation {
        row: temporal_row,
        identity: Some(decode_write_identity(row, TABLE)?),
    })
}

/// Read the row's attribute family from its stored code.
fn decode_attribute_kind(row: &PgRow) -> Result<AttributeKind, TemporalStoreError> {
    let stored: i16 = row.try_get("attribute_kind")?;
    Ok(AttributeKind::try_from(stored).map_err(StoredHistoryError::AttributeKind)?)
}

/// Read the row's expanded attribute IRI.
pub(crate) fn decode_attribute_name(row: &PgRow) -> Result<AttributeName, TemporalStoreError> {
    let raw: String = row.try_get("attribute_name")?;
    Ok(raw.parse::<AttributeName>().map_err(StoredHistoryError::AttributeName)?)
}

/// Read the stream's multi-attribute discriminator.
pub(crate) fn decode_dataset_id(row: &PgRow) -> Result<Option<Identifier>, TemporalStoreError> {
    Ok(row
        .try_get::<Option<String>, _>("dataset_id")?
        .map(|raw| raw.parse::<Identifier>().map_err(StoredHistoryError::DatasetId))
        .transpose()?)
}

/// Read the row's stored unit-code identity.
///
/// A stored value outside the range the catalog assigns fails the decode: it
/// cannot name a registration, so rendering the instance without its unit would
/// silently change what the value means.
pub(crate) fn decode_unit_code_id(row: &PgRow, column: UnitCodeColumn) -> Result<Option<UnitCodeId>, TemporalStoreError> {
    Ok(row
        .try_get::<Option<i16>, _>(<&'static str>::from(column))?
        .map(|stored| UnitCodeId::try_from(stored).map_err(|source| StoredHistoryError::UnitCodeId { column, source }))
        .transpose()?)
}

/// Read the row's metadata, as an instance of its family stores it.
fn decode_metadata(row: &PgRow, attribute_type: AttributeType, attribute_name: &AttributeName) -> Result<Option<ObservationMetadata>, TemporalStoreError> {
    let stored: Option<&RawValue> = row.try_get("metadata").unwrap_or(None);
    match stored {
        Some(stored) => Ok(ObservationMetadata::from_stored(attribute_type, attribute_name, stored.get())
            .map_err(|refusal| StoredHistoryError::Metadata(Box::new(refusal)))?),
        None => Ok(None),
    }
}

/// Read the row's value from the column its stored value kind names.
fn decode_value(row: &PgRow, attribute_type: AttributeType, attribute_name: &AttributeName) -> Result<TemporalValue, TemporalStoreError> {
    let stored: i16 = row.try_get("value_kind")?;
    let kind = ValueKind::try_from(stored).map_err(|source| StoredHistoryError::ValueKind { table: TABLE, source })?;
    match kind {
        ValueKind::Tombstone => Ok(TemporalValue::Tombstone {
            deleted_at: required_column(row, TABLE, HistoryColumn::DeletedAt)?,
        }),
        ValueKind::Number => Ok(TemporalValue::Number(decode_number(row)?)),
        ValueKind::String => Ok(TemporalValue::String(required_column(row, TABLE, HistoryColumn::ValueString)?)),
        ValueKind::Boolean => Ok(TemporalValue::Boolean(required_column(row, TABLE, HistoryColumn::ValueBoolean)?)),
        ValueKind::Geometry => Ok(TemporalValue::Geometry(decode_geometry(row, attribute_name)?)),
        ValueKind::Object => {
            let stored: &RawValue = required_column(row, TABLE, HistoryColumn::ValueObject)?;
            Ok(StructuredValue::from_stored(attribute_type, attribute_name, stored.get())
                .map(TemporalValue::Structured)
                .map_err(|refusal| StoredHistoryError::ValueObject(Box::new(refusal)))?)
        }
        ValueKind::DateTime => Ok(TemporalValue::DateTime(required_column(row, TABLE, HistoryColumn::ValueDateTime)?)),
        ValueKind::Uri => {
            let raw: String = required_column(row, TABLE, HistoryColumn::ValueUri)?;
            Ok(raw.parse::<Uri>().map(TemporalValue::Uri).map_err(StoredHistoryError::ValueUri)?)
        }
    }
}

/// Read the arbitrary-precision value column.
///
/// The stored column is `NUMERIC`, so its decimal is carried directly into the
/// exact neutral observation domain without a text or binary64 round trip
/// (`docs/exact-numeric-representation.md`): a literal the JSON numeric model
/// cannot carry fails the decode instead of becoming a rounded value or a null.
fn decode_number(row: &PgRow) -> Result<JsonNumber, TemporalStoreError> {
    let stored: BigDecimal = required_column(row, TABLE, HistoryColumn::ValueNumber)?;
    Ok(JsonNumber::from_decimal(&stored).map_err(StoredHistoryError::ValueNumber)?)
}

/// Read the geometry column, which the query layer selects as EWKB, and the
/// `bbox` and foreign members kept beside it (IETF RFC 7946 sections 5 and
/// 6.1).
fn decode_geometry(row: &PgRow, attribute_name: &AttributeName) -> Result<GeometryValue, TemporalStoreError> {
    let bytes: Vec<u8> = required_column(row, TABLE, HistoryColumn::ValueGeometryEwkb)?;
    let (form, _) = geometry_from_ewkb(&bytes)?.into_parts();
    let stored: Option<&RawValue> = row.try_get("value_geometry_members")?;
    let members = stored
        .map(|stored| ExpandedGeoJsonMembers::from_stored(attribute_name, stored.get()))
        .transpose()
        .map_err(|refusal| StoredHistoryError::ValueGeometryMembers(Box::new(refusal)))?
        .flatten();
    Ok(GeometryValue::with_members(form, members))
}

/// Build the geometry one stored EWKB value encodes.
fn geometry_from_ewkb(bytes: &[u8]) -> Result<Geometry, StoredHistoryError> {
    let ewkb = Ewkb::try_from(bytes).map_err(StoredHistoryError::ValueGeometry)?;
    Geometry::try_from(&ewkb).map_err(StoredHistoryError::ValueGeometry)
}

#[cfg(test)]
mod tests {
    use crate::history_row_decode::geometry_from_ewkb;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
    use serde_json::json;

    /// The bytes the write path binds for a geometry are the bytes the column
    /// hands back, so a geometry read back from them is the one written: its
    /// rings, its hole and the elevation of each elevated position.
    #[test]
    fn a_stored_ewkb_geometry_reads_back_as_the_geometry_written() {
        let written = serde_json::from_value::<Geometry>(json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0, 12.5], [4.0, 0.0, 12.5], [4.0, 4.0, 12.5], [0.0, 4.0, 12.5], [0.0, 0.0, 12.5]],
                [[1.0, 1.0, 3.0], [1.0, 2.0, 3.0], [2.0, 2.0, 3.0], [1.0, 1.0, 3.0]]
            ]
        }))
        .expect("a valid elevated Polygon with a hole");
        let bytes = geometry_bytes(Some(written.form()))
            .expect("the geometry encodes")
            .expect("a present geometry binds bytes");
        pretty_assertions::assert_eq!(geometry_from_ewkb(&bytes).expect("the stored bytes decode"), written);
    }

    #[test]
    fn stored_bytes_that_are_no_ewkb_are_refused() {
        assert!(geometry_from_ewkb(&[0x01, 0x02]).is_err());
    }
}
