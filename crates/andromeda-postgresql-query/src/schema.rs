//! The relation both port groups read entity state from.
//!
//! Only `entities` lives here: it is the one relation a context statement and
//! a temporal statement both name, so it is the one whose spelling has to be
//! agreed below them. Relations that belong to a single port group are
//! declared by the crate that assembles that group's statements, which is what
//! stops a backend replacing one group from inheriting the other's schema.

use sea_query::Iden;

/// The `entities` table - stores current entity state.
#[derive(Iden, Clone, Copy)]
pub enum Entities {
    /// Table name: `entities`
    Table,
    /// Primary key: entity URN identifier
    Id,
    /// Array of entity type IRIs
    Types,
    /// Full expanded JSON-LD payload
    Payload,
    /// Array of `ltree` scope paths an entity currently belongs to (clause 4.18).
    ScopePaths,
}

/// Alias `e` for entities table.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub struct E;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_table_name() {
        assert_eq!(Entities::Table.to_string(), "entities");
    }

    #[test]
    fn entities_column_names() {
        assert_eq!(Entities::Id.to_string(), "id");
        assert_eq!(Entities::Types.to_string(), "types");
        assert_eq!(Entities::Payload.to_string(), "payload");
    }

    #[test]
    fn the_entities_alias_is_e() {
        assert_eq!(E.to_string(), "e");
    }
}
