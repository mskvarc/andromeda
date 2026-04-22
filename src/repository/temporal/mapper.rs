//! Row-to-domain conversions for temporal data.

use crate::{
    core::error::AndromedaError,
    domain::{
        attribute_type::AttributeType,
        temporal::{TeaEntry, ValueType},
    },
};
use iref::IriBuf;
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;
use urn_rs::Urn;

/// Convert a `temporal_entity_attributes`-shaped row into a domain [`TeaEntry`].
pub fn row_to_tea(row: &PgRow) -> Result<TeaEntry, AndromedaError> {
    let attribute_name_raw: String = row.try_get("attribute_name")?;
    let attribute_name = IriBuf::new(attribute_name_raw.clone())
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.attribute_name '{attribute_name_raw}': {e}")))?;

    let attribute_type_raw: String = row.try_get("attribute_type")?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.attribute_type '{attribute_type_raw}': {e}")))?;

    let dataset_id = row.try_get::<Option<String>, _>("dataset_id")?.map(|raw| Urn::from_str(&raw)).transpose()?;

    let value_type_raw: String = row.try_get("value_type")?;
    let value_type = ValueType::from_str(&value_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.value_type '{value_type_raw}': {e}")))?;

    Ok(TeaEntry {
        attribute_name,
        attribute_type,
        dataset_id,
        value_type,
    })
}
