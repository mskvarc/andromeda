//! The column order of `registration_information`.

use crate::repository::{PinError, schema};
use andromeda_sql::declared_schema::column::DeclaredColumn;
use pretty_assertions::assert_eq;

/// A row-type value decodes field by field in the table's column order, so the
/// record the registration reader decodes keeps that order.
#[test]
fn an_information_record_follows_the_table_column_order() -> Result<(), PinError> {
    let schema = schema()?;
    let declared: Vec<&str> = schema
        .columns()
        .iter()
        .filter(|column| column.relation() == "registration_information")
        .map(DeclaredColumn::name)
        .collect();
    assert_eq!(
        declared,
        [
            "item_ordinal",
            "selector_ordinal",
            "entity_type_shape",
            "registration_id",
            "entity_types",
            "entity_id",
            "entity_id_pattern",
            "entity_id_pattern_form",
            "property_name_keys",
            "relationship_name_keys",
        ]
    );
    Ok(())
}
