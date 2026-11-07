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
//! Nothing in this module guesses. The history table records which typed column holds a row's value, so the value is read from that column and
//! from no other; a stored code the registry has never assigned fails the
//! decode rather than falling back to a default, because a row written by a
//! later release must be reported as unreadable instead of being silently
//! reinterpreted.

use crate::{
    error::TemporalStoreError,
    history_members::HistoryMembers,
    history_row_identity::{decode_instance_identity, decode_write_identity, nullable_instant, required_column},
    stored_history_error::{HistoryColumn, HistoryTable, StoredHistoryError, UnitCodeColumn},
};
use andromeda_ngsi_ld_entity::geometry_value::{ExpandedGeoJsonMembers, GeometryValue};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    reference::NgsiLdReference,
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::{
    StreamExtent,
    TemporalHistoryRow,
    TemporalValue,
    evolution_instance::EvolutionInstance,
    structured_value::StructuredValue,
    text_annotation::TextAnnotation,
};
use andromeda_ngsi_ld_number::{JsonNumber, declared_number::DeclaredNumber};
use andromeda_postgresql_attribute_instance::{
    name_keys::{HeldKeys, KeyNames},
    name_resolution::names_of,
    stored_json::StoredJson,
    stored_number::StoredNumber,
    sub_attribute::{SubAttributeElement, SubAttributeRecord},
};
use andromeda_postgresql_codes::{
    attribute_kind::AttributeKind,
    attribute_kind_code::attribute_type_of,
    stored_unit_code::unit_code_from_stored,
    value_kind::ValueKind,
};
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::{dataset_key::DatasetKey, entity_type_name_key::EntityTypeNameKey, history_source::grain::HistoryGrain};
use andromeda_temporal::port::history_reader::TemporalHistoryObservation;
use cefact_units::UnitCode;
use langtag::LangTagBuf;
use sea_query::Iden;
use serde_json::value::RawValue;
use sqlx::{Row, postgres::PgRow, types::Json};

/// The table every row this module decodes comes from.
const TABLE: HistoryTable = HistoryTable::AttributeHistory;

/// One `attribute_history` row read back but for its metadata, whose names
/// the read looks up for every row at once.
pub(crate) struct DecodedHistoryRow {
    /// The observation, its metadata not yet assembled.
    observation: TemporalHistoryObservation,
    /// What the row stores beside its value, every name still a key.
    members: HistoryMembers,
    /// The key of the datatype the row's literal declares.
    datatype_key: Option<EntityTypeNameKey>,
}

impl DecodedHistoryRow {
    /// Add every key the row's members and value hold to `held`.
    pub(crate) fn collect_keys(&self, held: &mut HeldKeys) {
        self.members.collect_keys(held);
        held.types.extend(self.datatype_key);
    }

    /// The observation, its metadata and its value's datatype assembled
    /// through `names`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when a key has no name in `names` or the
    /// members do not assemble.
    pub(crate) fn assemble(self, names: &KeyNames) -> Result<TemporalHistoryObservation, TemporalStoreError> {
        let DecodedHistoryRow {
            mut observation,
            members,
            datatype_key,
        } = self;
        observation.row.metadata = members.metadata(&observation.row.attribute_name, names)?;
        if let Some(key) = datatype_key {
            let datatype = names.type_name(key).cloned().ok_or(StoredHistoryError::ValueDatatype(key))?; // clone: the value owns its datatype
            observation.row.value = typed(observation.row.value, datatype)?;
        }
        Ok(observation)
    }
}

/// `value` carrying the datatype its literal declares.
///
/// A typed scalar is read without its datatype, whose name the dictionary
/// gives back once every row is read; the check constraints keep a datatype
/// off every other value and off a language-tagged string.
fn typed(value: TemporalValue, datatype: NgsiLdReference) -> Result<TemporalValue, StoredHistoryError> {
    Ok(match value {
        TemporalValue::Number { number, .. } => TemporalValue::Number {
            number,
            datatype: Some(datatype),
        },
        TemporalValue::Boolean { flag, .. } => TemporalValue::Boolean {
            flag,
            datatype: Some(datatype),
        },
        TemporalValue::DeclaredNumber { number, .. } => TemporalValue::DeclaredNumber {
            number,
            datatype: Some(datatype),
        },
        TemporalValue::DeclaredBoolean { flag, .. } => TemporalValue::DeclaredBoolean {
            flag,
            datatype: Some(datatype),
        },
        TemporalValue::DateTime { instant, .. } => TemporalValue::DateTime {
            instant,
            datatype: Some(datatype),
        },
        TemporalValue::Date { date, .. } => TemporalValue::Date {
            date,
            datatype: Some(datatype),
        },
        TemporalValue::Time { time, .. } => TemporalValue::Time {
            time,
            datatype: Some(datatype),
        },
        TemporalValue::String {
            text,
            annotation: TextAnnotation::Plain,
        } => TemporalValue::String {
            text,
            annotation: TextAnnotation::Datatype(datatype),
        },
        TemporalValue::String {
            annotation: TextAnnotation::Datatype(_) | TextAnnotation::Language(_),
            ..
        } => return Err(StoredHistoryError::DatatypeOnUntypedValue(ValueKind::String)),
        TemporalValue::Geometry(_) => return Err(StoredHistoryError::DatatypeOnUntypedValue(ValueKind::Geometry)),
        TemporalValue::Structured(_) => return Err(StoredHistoryError::DatatypeOnUntypedValue(ValueKind::Object)),
        TemporalValue::Uri(_) => return Err(StoredHistoryError::DatatypeOnUntypedValue(ValueKind::Uri)),
        TemporalValue::Scopes(_) | TemporalValue::Tombstone { .. } => return Err(StoredHistoryError::DatatypeOnUntypedValue(ValueKind::Tombstone)),
    })
}

/// The observations `rows` of one read hold, the names of every key their
/// members hold looked up once per dictionary.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when a row does not decode, a dictionary
/// cannot be read, or a row's members do not assemble.
pub(crate) async fn assembled_history(
    rows: &[PgRow],
    attribute_names: &dyn NameLookup<AttributeName>,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
) -> Result<Vec<TemporalHistoryObservation>, TemporalStoreError> {
    let decoded = rows.iter().map(decode_history_row).collect::<Result<Vec<_>, _>>()?;
    assemble_history(decoded, attribute_names, entity_type_names).await
}

/// The observations `decoded`, the names of every key their members hold
/// looked up once per dictionary.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when a dictionary cannot be read or a row's
/// members do not assemble.
async fn assemble_history(
    decoded: Vec<DecodedHistoryRow>,
    attribute_names: &dyn NameLookup<AttributeName>,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
) -> Result<Vec<TemporalHistoryObservation>, TemporalStoreError> {
    let mut held = HeldKeys::default();
    for row in &decoded {
        row.collect_keys(&mut held);
    }
    let names = names_of(&held.distinct(), attribute_names, entity_type_names).await?;
    decoded.into_iter().map(|row| row.assemble(&names)).collect()
}

/// Decode one `attribute_history` row into a [`TemporalHistoryRow`] whose
/// metadata is assembled once the read has the names of its keys.
pub(crate) fn decode_history_row(row: &PgRow) -> Result<DecodedHistoryRow, TemporalStoreError> {
    let attribute_type = attribute_type_of(decode_attribute_kind(row)?);
    let attribute_name = decode_attribute_name(row)?;
    let value = decode_value(row, attribute_type, &attribute_name)?;
    let members = decode_members(row)?;
    let datatype_key = row.try_get::<Option<i32>, _>("value_datatype_key")?.map(EntityTypeNameKey::from);

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
        instance: EvolutionInstance::Attribute(decode_instance_identity(row, TABLE)?),
        metadata: None,
        stream_extent: decode_stream_extent(row)?,
    };
    Ok(DecodedHistoryRow {
        observation: TemporalHistoryObservation {
            row: temporal_row,
            identity: Some(decode_write_identity(row, TABLE)?),
        },
        members,
        datatype_key,
    })
}

/// Read the row's attribute family from its stored code.
fn decode_attribute_kind(row: &PgRow) -> Result<AttributeKind, TemporalStoreError> {
    let stored: i16 = row.try_get("attribute_kind")?;
    Ok(AttributeKind::try_from(stored).map_err(StoredHistoryError::AttributeKind)?)
}

/// Read whether the page the row was read in cut its stream.
///
/// Every history statement projects `stream_continues`: true when the stream
/// holds instances beyond the page, false on a read that pages no stream, and
/// `NULL` for a stream that fills no page. A column that is missing or does not
/// decode fails the read: answering it as a whole stream would answer a cut
/// page as complete.
pub(crate) fn decode_stream_extent(row: &PgRow) -> Result<StreamExtent, TemporalStoreError> {
    Ok(match row.try_get::<Option<bool>, _>("stream_continues")? {
        Some(true) => StreamExtent::Cut,
        Some(false) | None => StreamExtent::Whole,
    })
}

/// Read the row's expanded attribute IRI.
pub(crate) fn decode_attribute_name(row: &PgRow) -> Result<AttributeName, TemporalStoreError> {
    let raw: String = row.try_get("attribute_name")?;
    Ok(raw.parse::<AttributeName>().map_err(StoredHistoryError::AttributeName)?)
}

/// Read the `datasetId` the stream's grain key holds.
pub(crate) fn decode_dataset_id(row: &PgRow) -> Result<Option<Identifier>, TemporalStoreError> {
    Ok(DatasetKey::stored(row.try_get::<&str, _>(HistoryGrain::DatasetKey.unquoted())?)
        .dataset_id()
        .map_err(StoredHistoryError::DatasetId)?)
}

/// Read the unit code the row's stored identity names.
///
/// An identity this build does not list fails the decode: rendering the
/// instance without its unit would silently change what the value means.
pub(crate) fn decode_unit_code(row: &PgRow, column: UnitCodeColumn) -> Result<Option<UnitCode>, TemporalStoreError> {
    Ok(row
        .try_get::<Option<i16>, _>(<&'static str>::from(column))?
        .map(|stored| unit_code_in(column, stored))
        .transpose()?)
}

/// The unit code the identity `stored`, read from `column`, names.
fn unit_code_in(column: UnitCodeColumn, stored: i16) -> Result<UnitCode, StoredHistoryError> {
    unit_code_from_stored(stored).map_err(|source| StoredHistoryError::UnitCodeId { column, source })
}

/// Read what the row stores beside its value, every name as its key.
fn decode_members(row: &PgRow) -> Result<HistoryMembers, TemporalStoreError> {
    let sub_attributes = row
        .try_get::<Option<Vec<SubAttributeRecord>>, _>("sub_attributes")?
        .unwrap_or_default()
        .into_iter()
        .map(SubAttributeElement::try_from)
        .collect::<Result<_, _>>()
        .map_err(|error| StoredHistoryError::SubAttribute(Box::new(error)))?;
    Ok(HistoryMembers {
        value_type_key: row.try_get::<Option<i32>, _>("value_type_key")?.map(EntityTypeNameKey::from),
        object_type_keys: row
            .try_get::<Option<Vec<i32>>, _>("object_type_keys")?
            .unwrap_or_default()
            .into_iter()
            .map(EntityTypeNameKey::from)
            .collect(),
        unit_code: decode_unit_code(row, UnitCodeColumn::UnitCodeId)?,
        expires_at: row.try_get("expires_at")?,
        proof_seals: row.try_get::<Option<Json<Box<RawValue>>>, _>("proof_seals")?.map(StoredJson::from),
        sub_attributes,
    })
}

/// Read the row's value from the column its stored value kind names.
fn decode_value(row: &PgRow, attribute_type: AttributeType, attribute_name: &AttributeName) -> Result<TemporalValue, TemporalStoreError> {
    let stored: i16 = row.try_get("value_kind")?;
    let kind = ValueKind::try_from(stored).map_err(|source| StoredHistoryError::ValueKind { table: TABLE, source })?;
    match kind {
        ValueKind::Tombstone => Ok(TemporalValue::Tombstone {
            deleted_at: required_column(row, TABLE, HistoryColumn::DeletedAt)?,
        }),
        ValueKind::Number => Ok(TemporalValue::Number {
            number: decode_number(row)?,
            datatype: None,
        }),
        ValueKind::String => Ok(TemporalValue::String {
            text: required_column(row, TABLE, HistoryColumn::ValueString)?,
            annotation: match row.try_get::<Option<String>, _>("value_language")? {
                Some(tag) => TextAnnotation::Language(LangTagBuf::new(tag).map_err(StoredHistoryError::ValueLanguage)?),
                None => TextAnnotation::Plain,
            },
        }),
        ValueKind::Boolean => Ok(TemporalValue::Boolean {
            flag: required_column(row, TABLE, HistoryColumn::ValueBoolean)?,
            datatype: None,
        }),
        ValueKind::DeclaredNumber => {
            let stored: StoredNumber = required_column(row, TABLE, HistoryColumn::ValueNumber)?;
            Ok(TemporalValue::DeclaredNumber {
                number: DeclaredNumber::try_from(stored).map_err(StoredHistoryError::ValueNumber)?,
                datatype: None,
            })
        }
        ValueKind::DeclaredBoolean => Ok(TemporalValue::DeclaredBoolean {
            flag: required_column(row, TABLE, HistoryColumn::ValueBoolean)?,
            datatype: None,
        }),
        ValueKind::Geometry => Ok(TemporalValue::Geometry(decode_geometry(row, attribute_name)?)),
        ValueKind::Object => {
            let stored: &RawValue = required_column(row, TABLE, HistoryColumn::ValueObject)?;
            Ok(StructuredValue::from_stored(attribute_type, attribute_name, stored.get())
                .map(TemporalValue::Structured)
                .map_err(|refusal| StoredHistoryError::ValueObject(Box::new(refusal)))?)
        }
        ValueKind::DateTime => Ok(TemporalValue::DateTime {
            instant: required_column(row, TABLE, HistoryColumn::ValueDateTime)?,
            datatype: None,
        }),
        ValueKind::Date => {
            let text: String = required_column(row, TABLE, HistoryColumn::ValueString)?;
            Ok(TemporalValue::Date {
                date: text.parse().map_err(StoredHistoryError::ValueDate)?,
                datatype: None,
            })
        }
        ValueKind::Time => {
            let text: String = required_column(row, TABLE, HistoryColumn::ValueString)?;
            Ok(TemporalValue::Time {
                time: text.parse().map_err(StoredHistoryError::ValueTime)?,
                datatype: None,
            })
        }
        ValueKind::Uri => {
            let raw: String = required_column(row, TABLE, HistoryColumn::ValueUri)?;
            Ok(raw.parse::<Uri>().map(TemporalValue::Uri).map_err(StoredHistoryError::ValueUri)?)
        }
    }
}

/// The stored column is `NUMERIC`, so its decimal is carried directly into the exact neutral observation domain without a text or binary64 round trip: a value the JSON numeric model cannot carry, non-finite or past its range, fails the decode instead of becoming a rounded value or a null.
fn decode_number(row: &PgRow) -> Result<JsonNumber, TemporalStoreError> {
    let stored: StoredNumber = required_column(row, TABLE, HistoryColumn::ValueNumber)?;
    Ok(JsonNumber::try_from(stored).map_err(StoredHistoryError::ValueNumber)?)
}

/// Read the geometry column, which the query layer selects as EWKB, and the `bbox` and foreign members kept beside it (IETF RFC 7946 sections 5 and 6.1).
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
    use crate::{
        error::TemporalStoreError,
        history_members::HistoryMembers,
        history_row_decode::{DecodedHistoryRow, assemble_history, geometry_from_ewkb, unit_code_in},
        stored_history_error::{StoredHistoryError, UnitCodeColumn},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_identity::InstanceIdentity};
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
    use andromeda_postgresql_attribute_instance::{
        name_keys::KeyNames,
        stored_members::InstanceMembers,
        stored_value::StoredValue,
        sub_attribute::SubAttributeElement,
    };
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stored_unit_code::UnknownStoredUnitCode};
    use andromeda_postgresql_name_dictionary::name_lookup::{NameLookup, NameLookupError};
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{interned_name::InternedName, resolved_names::ResolvedNames},
    };
    use andromeda_temporal::port::history_reader::TemporalHistoryObservation;
    use chrono::{TimeZone, Utc};
    use sqlx::PgConnection;
    use std::{collections::HashMap, sync::Mutex};

    const SPEED: &str = "https://example.org/speed";

    /// A Boolean sub-attribute element of the attribute keyed `key`, under
    /// `parent`.
    fn element(parent: i32, key: i32) -> SubAttributeElement {
        SubAttributeElement {
            parent,
            members: InstanceMembers {
                attribute_name_key: AttributeNameKey::from(key),
                dataset_id: None,
                attribute_kind: AttributeKind::Property,
                value: StoredValue::Boolean { flag: true, datatype: None },
                value_type_key: None,
                object_type_keys: Vec::new(),
                unit_code: None,
                observed_at: None,
                expires_at: None,
                proof_seals: None,
            },
            created_at: None,
            modified_at: None,
        }
    }

    /// A decoded row of `speed` whose members hold `sub_attributes`.
    fn decoded(sub_attributes: Vec<SubAttributeElement>) -> DecodedHistoryRow {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        DecodedHistoryRow {
            observation: TemporalHistoryObservation {
                row: TemporalHistoryRow {
                    attribute_name: SPEED.parse().expect("an attribute name"),
                    attribute_type: AttributeType::Property,
                    dataset_id: None,
                    value: TemporalValue::Boolean { flag: true, datatype: None },
                    observed_at: None,
                    recorded_at,
                    created_at: Some(recorded_at),
                    modified_at: Some(recorded_at),
                    instance: EvolutionInstance::Attribute(InstanceIdentity::generate()),
                    metadata: None,
                    stream_extent: StreamExtent::Whole,
                },
                identity: None,
            },
            members: HistoryMembers {
                sub_attributes,
                ..HistoryMembers::default()
            },
            datatype_key: None,
        }
    }

    /// The names of the attribute keys 3 and 7.
    fn names() -> KeyNames {
        let name = |raw: &str| raw.parse::<AttributeName>().expect("an attribute name");
        KeyNames::from_pairs(
            [
                (AttributeNameKey::from(3), name("https://example.org/accuracy")),
                (AttributeNameKey::from(7), name("https://example.org/method")),
            ],
            [],
        )
    }

    #[test]
    fn a_history_row_decodes_its_sub_attributes_through_the_dictionary() {
        let observation = decoded(vec![element(0, 3), element(1, 7)]).assemble(&names()).expect("the members assemble");
        let parts = observation.row.metadata.expect("the sub-attributes are metadata").into_parts();
        let accuracy = parts.sub_attributes.get("https://example.org/accuracy").expect("accuracy");
        let method = accuracy.instances()[0].sub_attributes().get("https://example.org/method");
        assert!(method.is_some(), "{parts:?}");
    }

    #[test]
    fn an_element_key_without_a_name_is_a_store_fault() {
        let refused = decoded(vec![element(0, 99)]).assemble(&names()).expect_err("key 99 has no name");
        assert!(
            matches!(refused, TemporalStoreError::StoredHistory(StoredHistoryError::Members(_))),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn the_names_of_a_read_are_looked_up_once_per_dictionary() {
        let rows = (0..50).map(|_| decoded(vec![element(0, 3), element(1, 7)])).collect();
        let attributes = RecordingNames::default();
        let types = RecordingNames::default();
        assemble_history(rows, &attributes, &types).await.expect_err("the double names no key");
        pretty_assertions::assert_eq!(attributes.calls(), vec![vec![3, 7]]);
        pretty_assertions::assert_eq!(types.calls(), vec![Vec::<i32>::new()]);
    }

    /// A dictionary double that answers no name and records every key set it
    /// is asked for.
    #[derive(Debug, Default)]
    struct RecordingNames {
        /// The key sets `names_of` was asked for, in call order.
        asked: Mutex<Vec<Vec<i32>>>,
    }

    impl RecordingNames {
        fn calls(&self) -> Vec<Vec<i32>> {
            self.asked.lock().expect("the record is not poisoned").clone()
        }
    }

    #[async_trait::async_trait]
    impl<N: InternedName> NameLookup<N> for RecordingNames {
        async fn lookup(&self, _names: &[N]) -> Result<ResolvedNames<N>, NameLookupError> {
            Ok(ResolvedNames::default())
        }

        async fn resolve_or_create(&self, _connection: &mut PgConnection, _names: &[N]) -> Result<HashMap<N, N::Key>, NameLookupError> {
            Ok(HashMap::new())
        }

        async fn names_of(&self, keys: &[N::Key]) -> Result<HashMap<N::Key, N>, NameLookupError> {
            self.asked
                .lock()
                .expect("the record is not poisoned")
                .push(keys.iter().map(|key| N::key_value(*key)).collect());
            Ok(HashMap::new())
        }
    }
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

    /// An identity no code of this build holds was written by a newer
    /// library; rendering the instance without its unit would change what the
    /// value means.
    #[test]
    fn an_unknown_stored_unit_identity_fails_the_row_decode() {
        assert!(matches!(
            unit_code_in(UnitCodeColumn::UnitCodeId, i16::MAX),
            Err(StoredHistoryError::UnitCodeId {
                column: UnitCodeColumn::UnitCodeId,
                source: UnknownStoredUnitCode { stored: i16::MAX },
            })
        ));
    }
}
