//! The relations only a temporal statement reads.
//!
//! The temporal catalogue, the history hypertable and the attribute-name
//! dictionary are named by this crate alone. They sit here rather than in the
//! shared rendering base because a temporal backend that is not PostgreSQL
//! replaces this crate, and nothing it leaves behind should still be declaring
//! a hypertable.

use sea_query::Iden;

/// The `temporal_entity_attributes` table - tracks attribute metadata for temporal queries.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntityAttributes {
    /// Table name: `temporal_entity_attributes`
    Table,
    /// Primary key
    Id,
    /// Foreign key to entity
    EntityId,
    /// Foreign key to attribute_names
    AttributeNameId,
    /// Optional dataset identifier
    DatasetId,
}

/// The `attribute_history` table - stores historical attribute values.
#[derive(Iden, Clone, Copy)]
pub enum AttributeHistory {
    /// Table name: `attribute_history`
    Table,
    /// Foreign key to temporal_entity_attributes
    TeaId,
}

/// The `attribute_names` table - normalized attribute name storage.
#[derive(Iden, Clone, Copy)]
pub enum AttributeNames {
    /// Table name: `attribute_names`
    Table,
    /// Primary key
    Id,
    /// Expanded attribute IRI
    Name,
}

/// Alias `tea` for temporal_entity_attributes table.
#[derive(Iden, Clone, Copy)]
#[iden = "tea"]
pub struct Tea;

/// Alias `ah` for attribute_history table.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
pub struct Ah;

/// Alias `an` for attribute_names table.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub struct An;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_entity_attributes_names() {
        assert_eq!(TemporalEntityAttributes::Table.to_string(), "temporal_entity_attributes");
        assert_eq!(TemporalEntityAttributes::EntityId.to_string(), "entity_id");
        assert_eq!(TemporalEntityAttributes::AttributeNameId.to_string(), "attribute_name_id");
    }

    #[test]
    fn attribute_history_names() {
        assert_eq!(AttributeHistory::Table.to_string(), "attribute_history");
        assert_eq!(AttributeHistory::TeaId.to_string(), "tea_id");
    }

    #[test]
    fn attribute_names_names() {
        assert_eq!(AttributeNames::Table.to_string(), "attribute_names");
        assert_eq!(AttributeNames::Name.to_string(), "name");
    }

    #[test]
    fn table_aliases() {
        assert_eq!(Tea.to_string(), "tea");
        assert_eq!(Ah.to_string(), "ah");
        assert_eq!(An.to_string(), "an");
    }
}
