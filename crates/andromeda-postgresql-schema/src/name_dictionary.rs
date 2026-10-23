//! The per-tenant name dictionaries.
//!
//! `attribute_names` maps an expanded attribute IRI to the compact `INTEGER`
//! key the temporal catalogue, the history hypertable, and
//! `current_attributes` all reference; `entity_type_names`
//! does the same for expanded entity type IRIs, which
//! `entity_search.entity_type_name_keys` and
//! `entities.evolution_type_name_keys` carry as integer arrays. One
//! dictionary serves every relation that names an attribute or a type, so a
//! rename of the physical column moves with this iden.

use sea_query::Iden;

/// The `attribute_names` table - the dictionary of expanded attribute IRIs.
#[derive(Iden, Clone, Copy)]
pub enum AttributeNames {
    /// Table name: `attribute_names`
    Table,
    /// Compact integer key referenced by every relation that names an attribute
    AttributeNameKey,
    /// Expanded attribute IRI
    Name,
}

/// The `entity_type_names` table - the dictionary of expanded entity type IRIs.
#[derive(Iden, Clone, Copy)]
pub enum EntityTypeNames {
    /// Table name: `entity_type_names`
    Table,
    /// Compact integer key carried in every `entity_type_name_keys` array
    EntityTypeNameKey,
    /// Expanded entity type IRI
    Name,
}

#[cfg(test)]
mod tests {
    use crate::name_dictionary::{AttributeNames, EntityTypeNames};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn attribute_names_table_and_columns() {
        assert_eq!(AttributeNames::Table.to_string(), "attribute_names");
        assert_eq!(AttributeNames::AttributeNameKey.to_string(), "attribute_name_key");
        assert_eq!(AttributeNames::Name.to_string(), "name");
    }

    #[test]
    fn entity_type_names_table_and_columns() {
        assert_eq!(EntityTypeNames::Table.to_string(), "entity_type_names");
        assert_eq!(EntityTypeNames::EntityTypeNameKey.to_string(), "entity_type_name_key");
        assert_eq!(EntityTypeNames::Name.to_string(), "name");
    }
}
