//! Decoding one grouped aggregation row's group-key values (the `aggrGroupBy` extension).
//!
//! Each grouping term contributes one [`GroupKeyValue`], in order. Most keys derive from data the row already carries, so only the bucket and value-in-effect keys read a projected `group_key_N` column. A value-in-effect key holding a `DateTime` is read from its `group_key_N_instant` column and spelled as every emitted instant is. A SQL NULL column is the absent key (`@none`).

use crate::{aggregation_row_decode::projected_column, error::TemporalStoreError, stored_aggregation_error::StoredAggregationError};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier, time::rendering::instant_text};
use andromeda_ngsi_ld_model::temporal::GroupKeyValue;
use andromeda_ngsi_ld_query::temporal::grouping::{AggregationGrouping, bucket_term::BucketKind, entity_member::EntityMember, group_key::GroupKey};
use andromeda_postgresql_temporal_query::{
    aggregation_group::ObservedUnits,
    aggregation_grouping::{group_key_column, group_key_instant_column},
};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;

/// Decode the ordered group-key values of one grouped aggregation row.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a projected group-key column is
/// present but unreadable, and [`TemporalStoreError::StoredAggregation`] when a
/// promoted `unitCode` group's unit is not uniform.
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
                BucketKind::Calendar => optional_value(projected_column::<DateTime<Utc>>(row, &group_key_column(index))?.map(instant_text)),
                // A cyclic field (`date_part`) is a whole number (hour 0-23,
                // day-of-week 0-6, month 1-12) that renders as its integer.
                BucketKind::Cyclic => optional_value(projected_column::<f64>(row, &group_key_column(index))?.map(|field| format!("{field}"))),
            },
            (GroupKey::Path(_), None) => match projected_column::<DateTime<Utc>>(row, &group_key_instant_column(index))? {
                Some(instant) => GroupKeyValue::Value(instant_text(instant)),
                None => optional_value(projected_column::<String>(row, &group_key_column(index))?),
            },
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
        ObservedUnits::Uniform(Some(code)) => Ok(GroupKeyValue::Value(code.as_str().to_owned())),
        // A promoted unit facet is uniform per group by construction; a
        // non-uniform span here is a corrupt result rather than a value.
        ObservedUnits::Conflicting { .. } | ObservedUnits::UnitAndUnitless { .. } => Err(StoredAggregationError::NonUniformGroupUnit.into()),
    }
}
