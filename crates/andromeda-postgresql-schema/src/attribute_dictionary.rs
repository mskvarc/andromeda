//! The per-tenant attribute-name dictionary.
//!
//! `attribute_names` maps an expanded attribute IRI to the compact `INTEGER`
//! key the temporal catalogue, the history hypertable, and the
//! `current_attribute_values` projection all reference. One dictionary serves
//! every relation that names an attribute, so a rename of the physical column
//! moves with this iden.

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

#[cfg(test)]
mod tests {
    use crate::attribute_dictionary::AttributeNames;
    use sea_query::Iden;

    #[test]
    fn attribute_names_table_and_columns() {
        assert_eq!(AttributeNames::Table.to_string(), "attribute_names");
        assert_eq!(AttributeNames::Id.to_string(), "id");
        assert_eq!(AttributeNames::Name.to_string(), "name");
    }
}
