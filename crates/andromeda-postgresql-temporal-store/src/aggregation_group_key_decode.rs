//! Decoding one grouped aggregation row's group-key values (the `aggrGroupBy`
//! extension, study §4.6).
//!
//! Every grouping term contributes one [`GroupKeyValue`] to the row, in order.
//! Most keys are derived from data the row already carries — the `datasetId` it
//! decoded, the family and unit its span reported, the entity it belongs to —
//! so only the bucket and value-in-effect keys read a projected `group_key_N`
//! column. A column that is SQL NULL is the absent key (`@none`).

use crate::{aggregation_row_decode::projected_column, error::TemporalStoreError};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier, rfc3339::sub_second};
use andromeda_ngsi_ld_model::temporal::GroupKeyValue;
use andromeda_ngsi_ld_query::temporal::grouping::{AggregationGrouping, bucket_term::BucketKind, entity_member::EntityMember, group_key::GroupKey};
use andromeda_postgresql_temporal_query::{aggregation_group::ObservedUnits, aggregation_grouping::group_key_column};
use andromeda_unit_catalog::{unit_code_id::UnitCodeId, unit_code_registry::registry};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;

/// Decode the ordered group-key values of one grouped aggregation row.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Decode`] when a projected group-key column is
/// present but unreadable, or when a promoted `unitCode` identity is not in the
/// unit registry.
pub(crate) fn decode_group_keys(
    row: &PgRow,
    grouping: &AggregationGrouping,
    entity_id: &Identifier,
    dataset_id: Option<&Identifier>,
    attribute_type: AttributeType,
    units: ObservedUnits,
) -> Result<Vec<GroupKeyValue>, TemporalStoreError> {
    let mut keys = Vec::with_capacity(grouping.terms().len());
    for (index, term) in grouping.terms().iter().enumerate() {
        let value = match (&term.key, term.bucket) {
            (GroupKey::Member(EntityMember::DatasetId), _) => optional_value(dataset_id.map(ToString::to_string)),
            (GroupKey::Member(EntityMember::Id), _) => GroupKeyValue::Value(entity_id.to_string()),
            (GroupKey::Member(EntityMember::Type), _) => GroupKeyValue::Value(attribute_type.to_string()),
            (GroupKey::Member(EntityMember::UnitCode), _) => unit_group_value(units)?,
            // The scope is a per-entity constant that no key column projects; it
            // becomes distinguishing only under a cross-entity resource.
            (GroupKey::Member(EntityMember::Scope), _) => GroupKeyValue::Absent,
            (GroupKey::Path(_), Some(bucket)) => match bucket.kind() {
                BucketKind::Calendar => optional_value(projected_column::<DateTime<Utc>>(row, &group_key_column(index))?.map(sub_second)),
                // A cyclic field (`date_part`) is a whole number — hour 0-23,
                // day-of-week 0-6, month 1-12 — that renders as its integer.
                BucketKind::Cyclic => optional_value(projected_column::<f64>(row, &group_key_column(index))?.map(|field| format!("{field}"))),
            },
            (GroupKey::Path(_), None) => optional_value(projected_column::<String>(row, &group_key_column(index))?),
        };
        keys.push(value);
    }
    Ok(keys)
}

/// A present group value or the absent-key marker.
fn optional_value(value: Option<String>) -> GroupKeyValue {
    match value {
        Some(value) => GroupKeyValue::Value(value),
        None => GroupKeyValue::Absent,
    }
}

/// Render a promoted `unitCode` group's uniform unit as its UN/CEFACT code.
///
/// The `GROUP BY` makes the unit uniform within each group, so the span is
/// `Uniform`: a present unit renders as its code, and an unannotated group is
/// the absent key.
fn unit_group_value(units: ObservedUnits) -> Result<GroupKeyValue, TemporalStoreError> {
    match units {
        ObservedUnits::Uniform(None) => Ok(GroupKeyValue::Absent),
        ObservedUnits::Uniform(Some(id)) => Ok(GroupKeyValue::Value(unit_code_text(id)?)),
        // A promoted unit facet is uniform per group by construction; a
        // non-uniform span here is a corrupt result rather than a value.
        ObservedUnits::Conflicting { .. } | ObservedUnits::UnitAndUnitless { .. } => Err(TemporalStoreError::Decode(
            "a unitCode grouping key produced a non-uniform unit span".to_owned(),
        )),
    }
}

/// The UN/CEFACT common code one unit identity renders to.
fn unit_code_text(id: UnitCodeId) -> Result<String, TemporalStoreError> {
    let registry = registry().map_err(|error| TemporalStoreError::Decode(format!("unit registry unavailable: {error}")))?;
    let code = registry
        .unit_code_of(id)
        .map_err(|error| TemporalStoreError::Decode(format!("unitCode grouping key {id}: {error}")))?;
    Ok(code.as_str().to_owned())
}
