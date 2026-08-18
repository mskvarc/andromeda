//! The per-tenant name dictionaries.
//!
//! `attribute_names` maps an expanded attribute IRI to the compact `INTEGER`
//! key the temporal catalogue, the history hypertable, and the
//! `current_attribute_values` projection all reference; `entity_type_names`
//! does the same for expanded entity type IRIs, which `entities.type_ids` and
//! `temporal_entities.type_ids` carry as integer arrays. One dictionary serves
//! every relation that names an attribute or a type, so a rename of the
//! physical column moves with this iden.

use sea_query::Iden;

/// The `attribute_names` table - the dictionary of expanded attribute IRIs.
#[derive(Iden, Clone, Copy)]
pub enum AttributeNames {
    /// Table name: `attribute_names`
    Table,
    /// Compact integer key referenced by every relation that names an attribute
    Id,
    /// Expanded attribute IRI
    Name,
}

/// The `entity_type_names` table - the dictionary of expanded entity type IRIs.
#[derive(Iden, Clone, Copy)]
pub enum EntityTypeNames {
    /// Table name: `entity_type_names`
    Table,
    /// Compact integer key carried in every `type_ids` array
    Id,
    /// Expanded entity type IRI
    Name,
}

#[cfg(test)]
mod tests {
    use crate::name_dictionary::{AttributeNames, EntityTypeNames};
    use sea_query::Iden;

    #[test]
    fn attribute_names_table_and_columns() {
        assert_eq!(AttributeNames::Table.to_string(), "attribute_names");
        assert_eq!(AttributeNames::Id.to_string(), "id");
        assert_eq!(AttributeNames::Name.to_string(), "name");
    }

    #[test]
    fn entity_type_names_table_and_columns() {
        assert_eq!(EntityTypeNames::Table.to_string(), "entity_type_names");
        assert_eq!(EntityTypeNames::Id.to_string(), "id");
        assert_eq!(EntityTypeNames::Name.to_string(), "name");
    }
}
