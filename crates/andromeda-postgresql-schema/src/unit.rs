//! The `unit_codes` catalog relation.
//!
//! The catalog of `unitCode` identities (clause 4.5.2) a stored attribute
//! instance's `unit_code_id` refers to. The hot write path carries no foreign
//! key to it, so an audit join is what checks a stored identity against it.

use sea_query::Iden;

/// The `unit_codes` table - the catalog of registered `unitCode` identities.
#[derive(Iden, Clone, Copy)]
pub enum UnitCodes {
    /// Table name: `unit_codes`
    Table,
    /// Surrogate identity a stored `unit_code_id` refers to
    Id,
}

#[cfg(test)]
mod tests {
    use crate::unit::UnitCodes;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn unit_codes_table_and_columns() {
        assert_eq!(UnitCodes::Table.to_string(), "unit_codes");
        assert_eq!(UnitCodes::Id.to_string(), "id");
    }
}
