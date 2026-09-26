//! The key the `entity_type_names` dictionary hands out for an expanded type IRI.

use derive_more::From;

/// Dictionary key for an expanded entity type IRI in the `entity_type_names`
/// table.
///
/// `entity_search.entity_type_name_keys` and
/// `temporal_entities.entity_type_name_keys` carry an `INTEGER[]` of these keys
/// instead of the type IRIs themselves, so a type selection
/// (clause 4.17) is a containment test over four-byte elements and the GIN
/// index that serves it stays a fraction of the size an IRI array's would be.
/// It is a PostgreSQL schema artefact and names nothing in ETSI GS CIM 009,
/// which is why it lives beside the statements that bind it rather than in the
/// data model.
///
/// The key is `INTEGER` for the same reason the attribute-name key is:
/// entity type IRIs are open-world domain values, so a `SMALLINT` ceiling
/// would be a capacity accident rather than a modelled constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, From)]
pub struct EntityTypeNameKey(i32);

impl EntityTypeNameKey {
    /// The underlying `INTEGER` value, for SQL binding.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_type_name_key::EntityTypeNameKey;

    #[test]
    fn a_type_dictionary_key_round_trips_through_its_integer() {
        pretty_assertions::assert_eq!(EntityTypeNameKey::from(7_i32).get(), 7_i32);
    }
}
