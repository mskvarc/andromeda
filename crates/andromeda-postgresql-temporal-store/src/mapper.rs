//! Row-to-domain conversions for temporal data.

use crate::{entry::TeaEntry, error::TemporalStoreError, value_type::ValueType};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

/// Convert a `temporal_entity_attributes`-shaped row into a domain [`TeaEntry`].
pub fn row_to_tea(row: &PgRow) -> Result<TeaEntry, TemporalStoreError> {
    let attribute_name_raw: String = row.try_get("attribute_name").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let attribute_name = attribute_name_raw
        .parse::<AttributeName>()
        .map_err(|e| TemporalStoreError::Decode(format!("Invalid temporal_entity_attributes.attribute_name '{attribute_name_raw}': {e}")))?;

    let attribute_type_raw: String = row.try_get("attribute_type").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| TemporalStoreError::Decode(format!("Invalid temporal_entity_attributes.attribute_type '{attribute_type_raw}': {e}")))?;

    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")
        .map_err(|e| TemporalStoreError::Decode(e.to_string()))?
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|e| TemporalStoreError::Decode(format!("Invalid temporal_entity_attributes.dataset_id '{raw}': {e}")))
        })
        .transpose()?;

    let value_type_raw: String = row.try_get("value_type").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let value_type = ValueType::from_str(&value_type_raw)
        .map_err(|e| TemporalStoreError::Decode(format!("Invalid temporal_entity_attributes.value_type '{value_type_raw}': {e}")))?;

    Ok(TeaEntry {
        attribute_name,
        attribute_type,
        dataset_id,
        value_type,
    })
}
