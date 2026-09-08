//! One projected attribute instance, and the grain it is written at.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey, value_kind::ValueKind};
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The identity of one projected attribute instance.
///
/// Clause 4.5.5 identifies an attribute instance by the attribute it belongs
/// to and the `datasetId` it carries, with the absent `datasetId` naming the
/// default instance. The projection repeats that grain per entity, so the key
/// is these three parts and nothing else: the table's uniqueness constraint
/// treats two absent `datasetId` values as the same instance, which is what
/// makes the default instance singular.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProjectedAttributeKey {
    /// The entity the instance belongs to.
    pub entity_key: EntityKey,
    /// Dictionary key of the instance's expanded attribute IRI.
    pub attribute_name_id: AttributeNameId,
    /// The instance's `datasetId`, absent on the default instance.
    pub dataset_id: Option<Uri>,
}

/// The value one projected attribute instance currently holds.
///
/// There is no tombstone member, and that is the point: the current-state
/// projection records what an entity holds now, so an attribute instance that
/// has been deleted has no row rather than a row that says it is gone. The
/// null representation `urn:ngsi-ld:null` (clauses 4.5.7 and 4.5.8) is a
/// temporal-evolution concern and is never stored in a typed column here,
/// where it would be indistinguishable from a URI a client genuinely sent.
///
/// Each member names exactly one typed column, which the table's own check
/// constraint re-states: a row whose declared shape disagrees with the column
/// holding its value is unrepresentable on both sides of the boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectedAttributeValue {
    /// An arbitrary-precision number.
    Number(JsonNumber),
    /// A string that is not an `xsd:dateTime`.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// A geometry, in the broker's single geodetic reference system.
    Geometry(Geometry),
    /// A structured value: object, array, language map, JSON literal or list.
    Object(Value),
    /// A point in time, already truncated to the broker's microsecond resolution.
    DateTime(DateTime<Utc>),
    /// A URI, which is the shape a relationship object is projected in.
    Uri(Uri),
}

impl ProjectedAttributeValue {
    /// The code the row records for this value's shape.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            ProjectedAttributeValue::Number(_) => ValueKind::Number,
            ProjectedAttributeValue::String(_) => ValueKind::String,
            ProjectedAttributeValue::Boolean(_) => ValueKind::Boolean,
            ProjectedAttributeValue::Geometry(_) => ValueKind::Geometry,
            ProjectedAttributeValue::Object(_) => ValueKind::Object,
            ProjectedAttributeValue::DateTime(_) => ValueKind::DateTime,
            ProjectedAttributeValue::Uri(_) => ValueKind::Uri,
        }
    }
}

/// One row of the `current_attribute_values` projection.
#[derive(Clone, Debug)]
pub struct ProjectedAttributeRow {
    /// The instance this row projects.
    pub key: ProjectedAttributeKey,
    /// The NGSI-LD attribute family the instance was written as (clause 5.2.4).
    pub attribute_kind: AttributeKind,
    /// The value the instance currently holds.
    pub value: ProjectedAttributeValue,
    /// Catalog identity of the instance's `unitCode`, absent when the instance
    /// declares no unit (clause 5.2.5).
    pub unit_code_id: Option<UnitCodeId>,
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::row::ProjectedAttributeValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use chrono::{TimeZone, Utc};

    #[test]
    fn no_projected_value_declares_the_reserved_tombstone_shape() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let values = [
            ProjectedAttributeValue::Number(JsonNumber::from(1)),
            ProjectedAttributeValue::String("text".to_owned()),
            ProjectedAttributeValue::Boolean(true),
            ProjectedAttributeValue::Object(serde_json::json!({"a": 1})),
            ProjectedAttributeValue::DateTime(instant),
            ProjectedAttributeValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        for value in &values {
            assert_ne!(value.kind(), ValueKind::Tombstone, "{value:?} must not project as a deletion");
        }
    }

    #[test]
    fn every_value_shape_names_a_distinct_typed_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let kinds = [
            ProjectedAttributeValue::Number(JsonNumber::from(1)).kind(),
            ProjectedAttributeValue::String("text".to_owned()).kind(),
            ProjectedAttributeValue::Boolean(true).kind(),
            ProjectedAttributeValue::Object(serde_json::json!([])).kind(),
            ProjectedAttributeValue::DateTime(instant).kind(),
            ProjectedAttributeValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")).kind(),
        ];
        let mut codes: Vec<i16> = kinds.iter().map(|kind| ValueKind::code(*kind)).collect();
        codes.sort_unstable();
        codes.dedup();
        pretty_assertions::assert_eq!(codes.len(), kinds.len());
    }
}
