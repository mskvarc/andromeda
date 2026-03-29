//! Domain types for the NGSI-LD Temporal API (ETSI GS CIM 009).
//!
//! TeaEntry stays here (sqlx::FromRow).

use sqlx::FromRow;

/// A row from the temporal_entity_attributes table.
#[derive(Debug, Clone, FromRow)]
pub struct TeaEntry {
    #[allow(dead_code)]
    pub id: uuid::Uuid,
    #[allow(dead_code)]
    pub entity_id: String,
    #[allow(dead_code)]
    pub attribute_name: String,
    /// "Property", "Relationship", "GeoProperty", etc.
    pub attribute_type: String,
    pub dataset_id: Option<String>,
    /// "Number", "String", "Boolean", "Geometry", "Object", "DateTime", "Uri"
    pub value_type: String,
}
