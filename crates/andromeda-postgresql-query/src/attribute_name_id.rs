//! The key the `attribute_names` dictionary hands out for an expanded IRI.

use derive_more::From;

/// Dictionary key for an expanded attribute IRI in the `attribute_names` table.
///
/// `attribute_names` normalises attribute IRIs to an `INTEGER` so the temporal
/// catalogue, the history hypertable, and the `current_attribute_values`
/// projection can
/// all index on a four-byte key instead of a full IRI. It is a PostgreSQL schema
/// artefact and names nothing in ETSI GS CIM 009, which is why it lives beside
/// the statements that bind it rather than in the data model.
///
/// The key is `INTEGER` rather than `SMALLINT` because attribute IRIs are
/// open-world domain values: NGSI-LD sets no bound on how many distinct ones a
/// tenant may write, so a 32,767-entry ceiling would be a capacity accident
/// rather than a modelled constraint. Bounding dictionary growth is a policy
/// decision taken where names are created, not a width chosen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, From)]
pub struct AttributeNameId(i32);

impl AttributeNameId {
    /// The underlying `INTEGER` value, for SQL binding.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_name_id::AttributeNameId;

    #[test]
    fn a_dictionary_key_round_trips_through_its_integer() {
        pretty_assertions::assert_eq!(AttributeNameId::from(7_i32).get(), 7_i32);
    }

    /// The widened key has to carry values a `SMALLINT` never could; a
    /// dictionary that wrapped at 32,767 would silently alias two attribute
    /// IRIs onto one key.
    #[test]
    fn a_dictionary_key_carries_values_beyond_the_smallint_ceiling() {
        pretty_assertions::assert_eq!(AttributeNameId::from(2_000_000_000_i32).get(), 2_000_000_000_i32);
    }
}
