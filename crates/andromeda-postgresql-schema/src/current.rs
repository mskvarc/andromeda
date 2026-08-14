//! The relations that hold current entity state.
//!
//! The `entities` row and the `current_attribute_values` projection of its
//! attributes are the current-state grain a context statement and a temporal
//! statement both read, together with the `e` alias they read it under.

use sea_query::Iden;

/// The `entities` table - stores current entity state.
#[derive(Iden, Clone, Copy)]
pub enum Entities {
    /// Table name: `entities`
    Table,
    /// Surrogate primary key every owned child relation references
    EntityKey,
    /// Entity URN identifier, the external identity of the row
    Id,
    /// Array of entity type IRIs
    Types,
    /// Full expanded JSON-LD payload
    Payload,
    /// Array of `ltree` scope paths an entity currently belongs to (clause 4.18).
    ScopePaths,
}

/// The `current_attribute_values` table - one row per current attribute
/// instance of an entity.
///
/// The grain is the clause 4.5.5 dataset grain: an entity, an attribute name
/// and an optional `datasetId`. Exactly one of the seven typed value columns is
/// populated on any row, and which one is pinned to `value_kind`, so a
/// predicate that knows the value shape it is comparing knows the single column
/// it has to read.
///
/// A deleted attribute is an absent row rather than a stored marker: the
/// tombstone value kind belongs to history and never appears here, which is why
/// row presence alone answers an existence test.
#[derive(Iden, Clone, Copy)]
pub enum CurrentAttributeValues {
    /// Table name: `current_attribute_values`
    Table,
    /// Foreign key to `entities`, cascading on entity deletion
    EntityKey,
    /// Instant the projected instance was last written
    ModifiedAt,
    /// Foreign key to `attribute_names`
    AttributeNameId,
    /// Attribute family code of the projected instance
    AttributeKind,
    /// Value shape code, pinning which typed value column is populated
    ValueKind,
    /// DateTime value, when `value_kind` selects it
    ValueDatetime,
    /// Foreign key to `unit_codes` for the instance's `unitCode` (clause 4.5.2)
    UnitCodeId,
    /// Boolean value, when `value_kind` selects it
    ValueBool,
    /// Optional dataset identifier (clause 4.5.5)
    DatasetId,
    /// Numeric value, when `value_kind` selects it
    ValueNumber,
    /// String value, when `value_kind` selects it
    ValueString,
    /// Geometry value, when `value_kind` selects it
    ValueGeo,
    /// JSON object or array value, when `value_kind` selects it
    ValueObject,
    /// URI value, when `value_kind` selects it
    ValueUri,
}

/// Alias `e` for the entities table.
///
/// Shared by the context type predicate and the temporal stage-1 statement,
/// which both read the entities row under this alias.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub struct E;

#[cfg(test)]
mod tests {
    use crate::current::{CurrentAttributeValues, E, Entities};
    use sea_query::Iden;

    #[test]
    fn entities_table_name() {
        assert_eq!(Entities::Table.to_string(), "entities");
    }

    #[test]
    fn entities_column_names() {
        assert_eq!(Entities::EntityKey.to_string(), "entity_key");
        assert_eq!(Entities::Id.to_string(), "id");
        assert_eq!(Entities::Types.to_string(), "types");
        assert_eq!(Entities::Payload.to_string(), "payload");
        assert_eq!(Entities::ScopePaths.to_string(), "scope_paths");
    }

    #[test]
    fn current_attribute_values_table_name() {
        assert_eq!(CurrentAttributeValues::Table.to_string(), "current_attribute_values");
    }

    #[test]
    fn current_attribute_values_grain_column_names() {
        assert_eq!(CurrentAttributeValues::EntityKey.to_string(), "entity_key");
        assert_eq!(CurrentAttributeValues::AttributeNameId.to_string(), "attribute_name_id");
        assert_eq!(CurrentAttributeValues::DatasetId.to_string(), "dataset_id");
        assert_eq!(CurrentAttributeValues::ModifiedAt.to_string(), "modified_at");
        assert_eq!(CurrentAttributeValues::AttributeKind.to_string(), "attribute_kind");
        assert_eq!(CurrentAttributeValues::ValueKind.to_string(), "value_kind");
        assert_eq!(CurrentAttributeValues::UnitCodeId.to_string(), "unit_code_id");
    }

    #[test]
    fn current_attribute_values_typed_value_column_names() {
        assert_eq!(CurrentAttributeValues::ValueNumber.to_string(), "value_number");
        assert_eq!(CurrentAttributeValues::ValueString.to_string(), "value_string");
        assert_eq!(CurrentAttributeValues::ValueBool.to_string(), "value_bool");
        assert_eq!(CurrentAttributeValues::ValueGeo.to_string(), "value_geo");
        assert_eq!(CurrentAttributeValues::ValueObject.to_string(), "value_object");
        assert_eq!(CurrentAttributeValues::ValueDatetime.to_string(), "value_datetime");
        assert_eq!(CurrentAttributeValues::ValueUri.to_string(), "value_uri");
    }

    #[test]
    fn the_entities_alias_is_e() {
        assert_eq!(E.to_string(), "e");
    }
}
