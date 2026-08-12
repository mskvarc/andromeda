//! Mutation and retrieval of individual temporal attribute instances.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`, and that identifier is stored in one of two columns: the bare
//! UUID of a broker-minted identifier, or the verbatim text of one a client
//! supplied. Every statement here is therefore written twice, once per column,
//! so each reads the partial index that covers it instead of an `OR` the
//! planner has to resolve with a scan.

use crate::{
    attribute_instance::{collect_metadata, detect_attribute_type, extract_observed_at},
    attribute_kind_code::stored_attribute_kind,
    attribute_value::{AttributeValue, HistoryValueColumns, extract_attribute_value},
    error::TemporalStoreError,
    repo::TemporalRepository,
    unit_code_metadata::take_unit_code,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    uri::Uri,
};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use const_format::concatcp;
use serde_json::Value;
use sqlx::postgres::PgRow;

impl TemporalRepository {
    /// Replace one expanded attribute instance through the typed PostgreSQL update path.
    pub(crate) async fn update_attribute_instance(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        expanded: &Value,
    ) -> Result<(), TemporalStoreError> {
        let state = build_update_instance_state(expanded)?;
        self.update_instance(entity_id, attribute_name, instance, &state).await
    }

    /// Physically remove one named attribute instance from history.
    ///
    /// Clause 5.6.15 deletes the addressed instance itself rather than
    /// recording that it ended, so the row is removed and no tombstone is
    /// written: a tombstone would keep answering temporal queries for an
    /// instance the client asked to have removed.
    ///
    /// Deliberate retention asymmetry: unlike whole-entity temporal deletion,
    /// this single-instance path does not read the removed row's `write_id`
    /// and does not release the write record it belongs to. Every other row
    /// of that write (other instances, other attributes, current state,
    /// notifications, side effects) may still exist, so this statement cannot
    /// know from a single deleted row whether the write record it belonged to
    /// has become unreferenced. Leaving the write record retained is the safe
    /// direction: retention's own periodic sweep is what actually knows the
    /// full set of referents and reaps a write record once none of them
    /// remain, so retaining here costs one write record's worth of storage
    /// until that sweep runs, whereas guessing wrong and releasing early would
    /// corrupt replay identity for every sibling row still pointing at it.
    pub(crate) async fn delete_instance(&self, instance: &InstanceIdentity, entity_id: &Uri, attribute_name: &AttributeName) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(DELETE_GENERATED_INSTANCE_SQL).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(DELETE_VERBATIM_INSTANCE_SQL).bind(identifier.as_str()),
        };
        let deleted = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        // A broker-minted identifier never claims a row in the first place
        // (`duplicate_instance_claim`), so only a verbatim one has a claim to
        // release. Releasing it here is what lets a client reuse the
        // `instanceId` of an instance it has just deleted.
        if let InstanceIdentity::Verbatim(identifier) = instance {
            sqlx::query(RELEASE_VERBATIM_INSTANCE_CLAIM_SQL)
                .bind(identifier.as_str())
                .bind(entity_id.as_str())
                .bind(attribute_name.as_str())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Modify one named attribute instance in place.
    async fn update_instance(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        state: &UpdateInstanceState,
    ) -> Result<(), TemporalStoreError> {
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(UPDATE_GENERATED_INSTANCE_SQL).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(UPDATE_VERBATIM_INSTANCE_SQL).bind(identifier.as_str()),
        };
        let updated = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .bind(state.value_kind.code())
            .bind(state.columns.value_number.as_deref())
            .bind(state.columns.value_string.as_deref())
            .bind(state.columns.value_bool)
            .bind(geometry_bytes(state.columns.value_geo.as_ref())?)
            .bind(state.columns.value_object.as_ref())
            .bind(state.columns.value_datetime)
            .bind(state.columns.value_uri.as_ref().map(Uri::as_str))
            .bind(state.observed_at)
            .bind(state.metadata.as_ref())
            .bind(state.unit_code_id.map(UnitCodeId::get))
            .bind(stored_attribute_kind(state.attribute_type).code())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if updated == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        Ok(())
    }

    /// Fetch a single attribute instance for policy validation.
    pub(crate) async fn get_instance(
        &self,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<Option<PgRow>, TemporalStoreError> {
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(SELECT_GENERATED_INSTANCE_SQL).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(SELECT_VERBATIM_INSTANCE_SQL).bind(identifier.as_str()),
        };
        Ok(statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// Report an instance the addressed attribute does not hold.
fn instance_not_found(instance: &InstanceIdentity) -> String {
    format!("Attribute instance {} not found", instance.rendered(InstanceFamily::Attribute))
}

/// Owned update values kept alive while PostgreSQL binds borrow them.
struct UpdateInstanceState {
    /// Typed history columns.
    columns: HistoryValueColumns,
    /// Expanded instance metadata, without the `unitCode` member.
    metadata: Option<Value>,
    /// Catalog identity of the instance's `unitCode`, when it declares one.
    unit_code_id: Option<UnitCodeId>,
    /// The observation instant the modified instance declares, if any.
    observed_at: Option<DateTime<Utc>>,
    /// The value shape the stored row declares, which the history table
    /// constrains to agree with the column the value lands in.
    value_kind: ValueKind,
    /// The attribute family detected from the replacement instance.
    ///
    /// Clause 5.6.14 defines instance PATCH as replacement by the supplied
    /// complete fragment, so a replacement that changes family must move the
    /// stored `attribute_kind` along with the value columns: retaining it
    /// here is what lets the update statement keep the two in agreement
    /// rather than rendering the old family over the new value shape.
    attribute_type: AttributeType,
}

/// Decode one expanded instance into owned PostgreSQL update state.
fn build_update_instance_state(expanded: &Value) -> Result<UpdateInstanceState, TemporalStoreError> {
    let attribute_type = detect_attribute_type(expanded);
    let attribute_value = extract_attribute_value(expanded, attribute_type)?;
    let value_kind = value_kind_of(&attribute_value);
    let observed_at = extract_observed_at(expanded)?;
    let mut metadata = collect_metadata(expanded);
    let unit_code_id = take_unit_code(&mut metadata)?;
    Ok(UpdateInstanceState {
        columns: attribute_value.into_columns(),
        metadata,
        unit_code_id,
        observed_at,
        value_kind,
        attribute_type,
    })
}

/// The stored value shape of a decoded instance value.
///
/// A modified instance always carries a value, so no input to this mapping is a
/// [`ValueKind::Tombstone`]: that member is reserved for a row recording a
/// deletion (clauses 4.5.7 and 4.5.8), which clause 5.6.15 cannot produce.
const fn value_kind_of(value: &AttributeValue) -> ValueKind {
    match value {
        AttributeValue::Number(_) => ValueKind::Number,
        AttributeValue::String(_) => ValueKind::String,
        AttributeValue::Boolean(_) => ValueKind::Boolean,
        AttributeValue::Geometry(_) => ValueKind::Geometry,
        AttributeValue::Object(_) => ValueKind::Object,
        AttributeValue::DateTime(_) => ValueKind::DateTime,
        AttributeValue::Uri(_) => ValueKind::Uri,
    }
}

/// The streams of one entity's attribute, whatever `datasetId` they carry.
///
/// An `instanceId` is unique across the attribute, so the dataset dimension is
/// not part of the address clause 5.6.15 defines; every stream of the named
/// attribute is a candidate and the instance predicate selects within them.
const ATTRIBUTE_STREAMS_SUBQUERY: &str = r#"
    SELECT s.id
    FROM temporal_attribute_streams s
    JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
    JOIN attribute_names an ON an.id = s.attribute_name_id
    WHERE te.id = $2 AND an.name = $3
"#;

const DELETE_GENERATED_INSTANCE_SQL: &str = concatcp!(
    "DELETE FROM attribute_history WHERE generated_instance_uuid = $1 AND stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ")"
);

const DELETE_VERBATIM_INSTANCE_SQL: &str = concatcp!(
    "DELETE FROM attribute_history WHERE verbatim_instance_id = $1 AND stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ")"
);

/// Free the `(entity, attribute, verbatim instanceId)` claim a deleted
/// instance held, so a future write may reuse the identifier (clause 4.5.7
/// does not reserve a deleted instance's identity forever).
const RELEASE_VERBATIM_INSTANCE_CLAIM_SQL: &str = r#"
DELETE FROM attribute_history_instance_claims c
USING temporal_entities te, attribute_names an
WHERE c.verbatim_instance_id = $1
  AND c.temporal_entity_key = te.entity_key AND te.id = $2
  AND c.attribute_name_id = an.id AND an.name = $3
"#;

/// Value columns, declared shape, and the modification instant of one instance.
///
/// `modified_at` is the NGSI-LD `modifiedAt` axis (clause 4.8), which a
/// successful modification advances; `recorded_at` is the operational instant
/// the row was written and is never rewritten, so a modified row keeps sorting
/// where the write that created it placed it.
///
/// `$5` is the value's decimal literal, cast to the column's arbitrary-precision
/// type: a number that has no exact double is stored by a modification exactly
/// as a creation stores it.
///
/// `attribute_kind` is set alongside the value columns rather than left as it
/// was: clause 5.6.14 defines this operation as replacement by the supplied
/// complete fragment, and a replacement that changes family must move the
/// row's declared family with it, or the row would render under its old
/// family while storing the new family's value shape.
const UPDATE_INSTANCE_ASSIGNMENTS: &str = r#"
UPDATE attribute_history
SET value_kind = $4,
    value_number = $5::numeric,
    value_string = $6,
    value_bool = $7,
    value_geo = CASE WHEN $8 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($8)) ELSE NULL END,
    value_object = $9,
    value_datetime = $10,
    value_uri = $11,
    observed_at = $12,
    metadata = $13,
    unit_code_id = $14,
    attribute_kind = $15,
    modified_at = NOW()
"#;

const UPDATE_GENERATED_INSTANCE_SQL: &str = concatcp!(
    UPDATE_INSTANCE_ASSIGNMENTS,
    "WHERE generated_instance_uuid = $1 AND stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ")"
);

const UPDATE_VERBATIM_INSTANCE_SQL: &str = concatcp!(
    UPDATE_INSTANCE_ASSIGNMENTS,
    "WHERE verbatim_instance_id = $1 AND stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ")"
);

const SELECT_INSTANCE_COLUMNS: &str = r#"
SELECT ah.recorded_at, ah.write_id, ah.write_ordinal, ah.stream_id,
       ah.attribute_kind, ah.value_kind, ah.observed_at, ah.deleted_at
FROM attribute_history ah
"#;

const SELECT_GENERATED_INSTANCE_SQL: &str = concatcp!(
    SELECT_INSTANCE_COLUMNS,
    "WHERE ah.generated_instance_uuid = $1 AND ah.stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ") LIMIT 1"
);

const SELECT_VERBATIM_INSTANCE_SQL: &str = concatcp!(
    SELECT_INSTANCE_COLUMNS,
    "WHERE ah.verbatim_instance_id = $1 AND ah.stream_id IN (",
    ATTRIBUTE_STREAMS_SUBQUERY,
    ") LIMIT 1"
);

#[cfg(test)]
mod tests {
    use crate::{
        attribute_value::AttributeValue,
        instance_mutation::{
            DELETE_GENERATED_INSTANCE_SQL,
            DELETE_VERBATIM_INSTANCE_SQL,
            RELEASE_VERBATIM_INSTANCE_CLAIM_SQL,
            SELECT_GENERATED_INSTANCE_SQL,
            SELECT_VERBATIM_INSTANCE_SQL,
            UPDATE_GENERATED_INSTANCE_SQL,
            UPDATE_VERBATIM_INSTANCE_SQL,
            value_kind_of,
        },
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_model::attribute::json_number::JsonNumber;
    use andromeda_postgresql_codes::value_kind::ValueKind;

    #[test]
    fn each_statement_addresses_exactly_one_instance_identity_column() {
        for statement in [DELETE_GENERATED_INSTANCE_SQL, UPDATE_GENERATED_INSTANCE_SQL, SELECT_GENERATED_INSTANCE_SQL] {
            assert!(statement.contains("generated_instance_uuid = $1"));
            assert!(!statement.contains("verbatim_instance_id"));
        }
        for statement in [DELETE_VERBATIM_INSTANCE_SQL, UPDATE_VERBATIM_INSTANCE_SQL, SELECT_VERBATIM_INSTANCE_SQL] {
            assert!(statement.contains("verbatim_instance_id = $1"));
            assert!(!statement.contains("generated_instance_uuid"));
        }
    }

    #[test]
    fn every_statement_is_confined_to_the_named_entity_and_attribute() {
        for statement in [
            DELETE_GENERATED_INSTANCE_SQL,
            DELETE_VERBATIM_INSTANCE_SQL,
            UPDATE_GENERATED_INSTANCE_SQL,
            UPDATE_VERBATIM_INSTANCE_SQL,
            SELECT_GENERATED_INSTANCE_SQL,
            SELECT_VERBATIM_INSTANCE_SQL,
        ] {
            assert!(statement.contains("FROM temporal_attribute_streams s"));
            assert!(statement.contains("WHERE te.id = $2 AND an.name = $3"));
            assert!(!statement.contains("temporal_entity_attributes"));
        }
    }

    #[test]
    fn a_modification_advances_the_modification_axis_and_leaves_the_write_clock_alone() {
        for statement in [UPDATE_GENERATED_INSTANCE_SQL, UPDATE_VERBATIM_INSTANCE_SQL] {
            assert!(statement.contains("modified_at = NOW()"));
            assert!(!statement.contains("recorded_at ="));
            assert!(statement.contains("value_kind = $4"));
        }
    }

    /// Clause 5.6.14 defines instance PATCH as replacement by the supplied
    /// complete fragment: a replacement that changes attribute family must
    /// move `attribute_kind` along with the value columns, or the row would
    /// keep rendering under its old family while storing the new family's
    /// value shape.
    #[test]
    fn a_modification_updates_the_stored_attribute_kind_alongside_the_value_columns() {
        for statement in [UPDATE_GENERATED_INSTANCE_SQL, UPDATE_VERBATIM_INSTANCE_SQL] {
            assert!(statement.contains("attribute_kind = $15"), "{statement}");
            assert!(statement.contains("value_kind = $4"), "{statement}");
        }
    }

    #[test]
    fn a_deletion_removes_the_row_rather_than_recording_a_tombstone() {
        for statement in [DELETE_GENERATED_INSTANCE_SQL, DELETE_VERBATIM_INSTANCE_SQL] {
            assert!(statement.starts_with("DELETE FROM attribute_history"));
            assert!(!statement.contains("deleted_at"));
        }
    }

    /// Deleting a verbatim instance must free the identifier it claimed, or a
    /// client could never reuse an `instanceId` it had already removed.
    /// Resolved through the same entity-and-attribute scope every other
    /// verbatim statement here uses, not through `stream_id`, since the claim
    /// table carries no stream column.
    #[test]
    fn a_verbatim_deletion_releases_the_instance_claim_it_held() {
        assert!(RELEASE_VERBATIM_INSTANCE_CLAIM_SQL.contains("DELETE FROM attribute_history_instance_claims"));
        assert!(RELEASE_VERBATIM_INSTANCE_CLAIM_SQL.contains("c.verbatim_instance_id = $1"));
        assert!(RELEASE_VERBATIM_INSTANCE_CLAIM_SQL.contains("te.id = $2"));
        assert!(RELEASE_VERBATIM_INSTANCE_CLAIM_SQL.contains("an.name = $3"));
    }

    /// The value column is `NUMERIC`, and the statement binds its decimal
    /// literal: a cast through `double precision` would round every clause
    /// 5.6.14 modification of an integer beyond 2^53.
    #[test]
    fn the_update_statement_binds_the_numeric_literal_without_a_double_hop() {
        for statement in [UPDATE_GENERATED_INSTANCE_SQL, UPDATE_VERBATIM_INSTANCE_SQL] {
            assert!(statement.contains("value_number = $5::numeric"), "{statement}");
            assert!(!statement.contains("double precision"), "{statement}");
        }
    }

    #[test]
    fn every_decoded_value_shape_maps_to_its_own_stored_kind() {
        let geometry = Geometry::from_geojson_value(&serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]}))
            .map(|geometry| value_kind_of(&AttributeValue::Geometry(geometry)))
            .map_err(|error| error.to_string());
        assert_eq!(geometry, Ok(ValueKind::Geometry));
        let kinds = [
            value_kind_of(&AttributeValue::Number(JsonNumber::Signed(1))),
            value_kind_of(&AttributeValue::String("text".to_owned())),
            value_kind_of(&AttributeValue::Boolean(true)),
            value_kind_of(&AttributeValue::Object(serde_json::json!({}))),
        ];
        assert_eq!(kinds, [ValueKind::Number, ValueKind::String, ValueKind::Boolean, ValueKind::Object]);
        assert!(!kinds.contains(&ValueKind::Tombstone));
    }
}
